//! The synchronisation primitives, exercised as a guest drives them.
//!
//! # Why these are worth the effort
//!
//! Every one of them is an object the guest creates in one call and uses in another, so the
//! failure mode is never a wrong return value in isolation - it is two guest threads inside
//! the same critical section, and the corruption that follows gets attributed to whatever
//! the lock was protecting rather than to here. A synchronisation bug is the furthest thing
//! in this codebase from where it shows up.
//!
//! # Handles are per-object, so these tests do not collide
//!
//! The tables are process-wide, but every `create_*` mints a fresh handle, so a test that
//! makes its own objects cannot be disturbed by one running beside it. Nothing here
//! enumerates a table or destroys a handle it did not create, which is what keeps that
//! true.
//!
//! # Thread handles are just numbers
//!
//! `ThreadHandle` is a `u64` the caller supplies, so ownership rules can be tested without
//! spawning anything. Where a test genuinely needs two threads - blocking, waking, barrier
//! rounds - it spawns them and says why.

use orbistoun_kernel::sync;
use std::time::Duration;

/// Two thread identities that are not each other and not [`sync::NO_MUTEX`]'s zero.
const ALICE: u64 = 0x1001;
const BOB: u64 = 0x2002;

/// Long enough that a wake genuinely had to travel between threads, short enough that a
/// broken test fails rather than hangs.
const PATIENCE: Duration = Duration::from_secs(5);

// --- mutexes -----------------------------------------------------------------------------

/// A fresh mutex is a real handle with the name it was given.
///
/// The handle is a block this crate owns rather than a small integer, so that a guest
/// reading a field through it finds memory instead of faulting.
#[test]
fn a_created_mutex_has_a_handle_and_remembers_its_name() {
    let m = sync::create(sync::Recursion::Forbidden, "render-queue");
    assert_ne!(m, sync::NO_MUTEX, "zero means nothing here");
    assert_eq!(sync::name_of(m).as_deref(), Some("render-queue"));
    assert!(sync::destroy(m));
}

/// A lock is taken and released by its owner.
#[test]
fn a_mutex_is_taken_and_released_by_its_owner() {
    let m = sync::create(sync::Recursion::Forbidden, "m");
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(sync::unlock(m, ALICE), Some(true));
    // And is free again afterwards, which is the half a leak would pass without.
    assert_eq!(
        sync::acquire(m, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(sync::unlock(m, BOB), Some(true));
    sync::destroy(m);
}

/// Re-locking a non-recursive mutex is refused **without blocking**.
///
/// Waiting there would deadlock against ourselves and look like a hang in the guest - so
/// the test that matters is not just that it answers `false`, but that it *answers at all*.
/// A test asserting the return value alone would hang instead of failing if this regressed,
/// which is why the call happens on a thread with a deadline on it.
#[test]
fn re_locking_a_non_recursive_mutex_is_refused_rather_than_deadlocked() {
    let m = sync::create(sync::Recursion::Forbidden, "m");
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(sync::acquire(m, ALICE, sync::Blocking::Forever));
    });
    assert_eq!(
        rx.recv_timeout(PATIENCE),
        Ok(Some(sync::Acquisition::Busy)),
        "the owner asking again must be told no, not made to wait"
    );

    assert_eq!(sync::unlock(m, ALICE), Some(true));
    sync::destroy(m);
}

/// A recursive mutex counts its acquisitions and must be released as many times.
///
/// The depth is the whole difference between the two kinds, and it is invisible until the
/// last release: a recursive lock that forgot to count would look correct right up to the
/// point another thread was let in one release early.
#[test]
fn a_recursive_mutex_must_be_released_as_many_times_as_it_was_taken() {
    let m = sync::create(sync::Recursion::Allowed, "m");
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Never),
        Some(sync::Acquisition::Locked),
        "three deep"
    );

    // Still held after two releases, so nobody else may have it.
    assert_eq!(sync::unlock(m, ALICE), Some(true));
    assert_eq!(sync::unlock(m, ALICE), Some(true));
    assert_eq!(
        sync::acquire(m, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Busy),
        "one acquisition is still outstanding"
    );

    assert_eq!(sync::unlock(m, ALICE), Some(true));
    assert_eq!(
        sync::acquire(m, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Locked),
        "and now it is free"
    );
    sync::unlock(m, BOB);
    sync::destroy(m);
}

/// Unlocking a lock you do not hold is refused, whoever you are.
///
/// **Refusing rather than releasing is the point.** Releasing somebody else's lock would
/// let two guest threads into the same critical section, and nothing downstream would ever
/// point back here.
#[test]
fn unlocking_a_mutex_you_do_not_hold_is_refused() {
    let m = sync::create(sync::Recursion::Forbidden, "m");

    assert_eq!(
        sync::unlock(m, ALICE),
        Some(false),
        "nobody holds it at all"
    );

    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(sync::unlock(m, BOB), Some(false), "somebody else holds it");
    // And the real owner still does, which a wrongly-permissive unlock would have broken.
    assert_eq!(
        sync::acquire(m, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Busy)
    );

    assert_eq!(sync::unlock(m, ALICE), Some(true));
    sync::destroy(m);
}

/// An impatient acquisition never blocks, and tells the two kinds apart for the owner.
#[test]
fn an_impatient_take_answers_immediately_and_respects_recursion() {
    let strict = sync::create(sync::Recursion::Forbidden, "strict");
    let loose = sync::create(sync::Recursion::Allowed, "loose");

    assert_eq!(
        sync::acquire(strict, ALICE, sync::Blocking::Never),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(
        sync::acquire(strict, ALICE, sync::Blocking::Never),
        Some(sync::Acquisition::Busy),
        "the owner may not take a non-recursive lock twice"
    );
    assert_eq!(
        sync::acquire(strict, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Busy)
    );

    assert_eq!(
        sync::acquire(loose, ALICE, sync::Blocking::Never),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(
        sync::acquire(loose, ALICE, sync::Blocking::Never),
        Some(sync::Acquisition::Locked),
        "but may here"
    );
    assert_eq!(
        sync::acquire(loose, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Busy),
        "and BOB may not"
    );

    sync::unlock(strict, ALICE);
    sync::unlock(loose, ALICE);
    sync::unlock(loose, ALICE);
    sync::destroy(strict);
    sync::destroy(loose);
}

/// An error-checking mutex tells its owner a self-relock is a deadlock, distinct from busy.
///
/// The whole reason an acquisition reports three states rather than two: a normal lock the owner
/// re-takes is *busy*, an error-checking one is a *deadlock*, and the platform gives those two
/// different codes (015-sync/mutex-recursion, D416). To another thread it is still just busy.
#[test]
fn an_errorcheck_mutex_reports_a_self_relock_as_a_deadlock() {
    let m = sync::create(sync::Recursion::Errorcheck, "errorcheck");
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Never),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Never),
        Some(sync::Acquisition::Deadlock),
        "the owner re-taking it is a deadlock, not a nest and not a plain busy"
    );
    assert_eq!(
        sync::acquire(m, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Busy),
        "to another thread it is simply held"
    );
    sync::unlock(m, ALICE);
    sync::destroy(m);
}

/// A blocked acquirer is woken when the lock is released.
///
/// The one behaviour that cannot be tested without two threads: `lock` waiting, and the
/// release actually notifying rather than leaving the waiter to a timeout it does not have.
#[test]
fn a_waiting_thread_is_woken_when_the_mutex_is_released() {
    let m = sync::create(sync::Recursion::Forbidden, "m");
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );

    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let taken = sync::acquire(m, BOB, sync::Blocking::Forever);
        let _ = tx.send(taken);
    });

    // It must still be waiting, because ALICE has not let go.
    assert!(
        rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "BOB should not have got a lock ALICE is holding"
    );

    assert_eq!(sync::unlock(m, ALICE), Some(true));
    assert_eq!(
        rx.recv_timeout(PATIENCE),
        Ok(Some(sync::Acquisition::Locked)),
        "and now it may"
    );
    waiter.join().expect("the waiter finishes");

    sync::unlock(m, BOB);
    sync::destroy(m);
}

/// Every mutex call on a handle naming nothing answers "no such object".
///
/// Distinct from `Some(false)`, which is a real object refusing. A guest branches
/// differently on each, and collapsing them would make a destroyed handle look like a busy
/// lock - which a caller retries forever.
#[test]
fn a_mutex_handle_naming_nothing_is_not_the_same_as_a_refusal() {
    let m = sync::create(sync::Recursion::Forbidden, "m");
    assert!(sync::destroy(m), "destroying it once works");
    assert!(!sync::destroy(m), "and only once");

    assert_eq!(sync::acquire(m, ALICE, sync::Blocking::Forever), None);
    assert_eq!(sync::acquire(m, ALICE, sync::Blocking::Never), None);
    assert_eq!(sync::unlock(m, ALICE), None);
    assert_eq!(sync::name_of(m), None);
    assert_eq!(
        sync::acquire(sync::NO_MUTEX, ALICE, sync::Blocking::Forever),
        None
    );
}

// --- semaphores ---------------------------------------------------------------------------

/// A semaphore hands out its initial count and then refuses.
#[test]
fn a_semaphore_hands_out_its_count_and_then_refuses() {
    let s = sync::create_semaphore(2, 8, "slots");
    assert_ne!(s, sync::NO_SEMAPHORE);
    assert_eq!(sync::semaphore_name_of(s).as_deref(), Some("slots"));

    assert_eq!(sync::semaphore_wait(s, sync::Blocking::Never), Some(true));
    assert_eq!(sync::semaphore_wait(s, sync::Blocking::Never), Some(true));
    assert_eq!(
        sync::semaphore_wait(s, sync::Blocking::Never),
        Some(false),
        "the count is spent"
    );

    assert_eq!(sync::semaphore_signal(s, 1), Some(true));
    assert_eq!(
        sync::semaphore_wait(s, sync::Blocking::Never),
        Some(true),
        "and returned"
    );
    assert!(sync::semaphore_destroy(s));
}

/// A handle is a counter, not a truncated pointer.
///
/// The handle is an `int`, and a host address truncated to four bytes collides with every
/// other semaphore sharing its low half - **silently**. Two created in a row differing by
/// exactly one is what a counter looks like and what a truncated address does not.
#[test]
fn semaphore_handles_are_counted_rather_than_derived_from_addresses() {
    let a = sync::create_semaphore(0, 1, "a");
    let b = sync::create_semaphore(0, 1, "b");
    assert_ne!(a, b);
    assert!(a > 0 && b > 0, "zero keeps meaning nothing here");
    sync::semaphore_destroy(a);
    sync::semaphore_destroy(b);
}

/// A signal past the ceiling is refused, not clamped.
///
/// **Silently capping would let a guest that has lost count carry on as though it had
/// not**, and the imbalance would surface as a hang somewhere with no connection to here.
#[test]
fn a_signal_past_the_ceiling_is_refused_rather_than_clamped() {
    let s = sync::create_semaphore(1, 3, "capped");

    assert_eq!(
        sync::semaphore_signal(s, 2),
        Some(true),
        "exactly to the ceiling"
    );
    assert_eq!(sync::semaphore_signal(s, 1), Some(false), "and no further");

    // Refused means unchanged: the three still there are all takeable, and no more.
    for _ in 0..3 {
        assert_eq!(sync::semaphore_wait(s, sync::Blocking::Never), Some(true));
    }
    assert_eq!(sync::semaphore_wait(s, sync::Blocking::Never), Some(false));

    // An addition that would not even fit in the counter is refused before the ceiling
    // comparison, rather than wrapping to a small number that passes it.
    assert_eq!(sync::semaphore_signal(s, u32::MAX), Some(false));
    assert_eq!(
        sync::semaphore_wait(s, sync::Blocking::Never),
        Some(false),
        "and nothing appeared"
    );

    sync::semaphore_destroy(s);
}

/// A waiter blocks until somebody signals.
#[test]
fn a_semaphore_waiter_blocks_until_it_is_signalled() {
    let s = sync::create_semaphore(0, 4, "empty");

    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = tx.send(sync::semaphore_wait(s, sync::Blocking::Forever));
    });

    assert!(
        rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "there is nothing to take yet"
    );
    assert_eq!(sync::semaphore_signal(s, 1), Some(true));
    assert_eq!(rx.recv_timeout(PATIENCE), Ok(Some(true)));
    waiter.join().expect("the waiter finishes");

    sync::semaphore_destroy(s);
}

/// A semaphore handle naming nothing answers "no such object".
#[test]
fn a_semaphore_handle_naming_nothing_answers_nothing() {
    let s = sync::create_semaphore(1, 1, "s");
    assert!(sync::semaphore_destroy(s));
    assert!(!sync::semaphore_destroy(s));

    assert_eq!(sync::semaphore_wait(s, sync::Blocking::Never), None);
    assert_eq!(sync::semaphore_signal(s, 1), None);
    assert_eq!(sync::semaphore_name_of(s), None);
    assert_eq!(
        sync::semaphore_wait(sync::NO_SEMAPHORE, sync::Blocking::Never),
        None
    );
}

// --- condition variables ----------------------------------------------------------------------

/// A signal arriving before anybody waits is remembered.
///
/// **Counted rather than relying on the host notify alone.** A guest may signal first, and
/// a wake that existed only as a host notification would be lost - leaving the next waiter
/// blocked on something that already happened.
#[test]
fn a_signal_before_anybody_waits_is_not_lost() {
    let c = sync::create_cond("ready");
    assert_eq!(sync::cond_name_of(c).as_deref(), Some("ready"));

    assert_eq!(sync::cond_signal(c), Some(true));
    assert_eq!(
        sync::cond_wait(c, Some(Duration::from_millis(50))),
        Some(true),
        "the owed wake is taken immediately"
    );
    // And it was consumed rather than left standing.
    assert_eq!(
        sync::cond_wait(c, Some(Duration::from_millis(50))),
        Some(false),
        "one signal is one wake"
    );

    assert!(sync::cond_destroy(c));
}

/// **One signal wakes one waiter, however many the notify reaches.**
///
/// The deterministic form of the bug a loaded test run found by accident. `cond_broadcast`
/// owes one wake and calls the host's `notify_all`, so both waiters are woken; the count is
/// what says only one of them was signalled. A wait that returned "signalled" for any wake -
/// which is what this did - let both through, and a guest would have two threads leaving a
/// condition only one of them was told about.
///
/// Nothing here forces a *spurious* wake, which cannot be provoked on demand. It provokes the
/// same thing by the route that can be: a real wake with nothing behind it.
#[test]
fn one_signal_releases_one_waiter_even_though_the_notify_reaches_both() {
    let c = sync::create_cond("one-of-two");
    let (tx, rx) = std::sync::mpsc::channel();
    for _ in 0..2 {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(sync::cond_wait(c, Some(Duration::from_millis(400))));
        });
    }
    drop(tx);

    // Long enough for both to be inside the wait before anything is owed.
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(sync::cond_broadcast(c), Some(true));

    let first = rx.recv_timeout(PATIENCE).expect("one waiter returns");
    let second = rx.recv_timeout(PATIENCE).expect("and so does the other");
    let mut both = [first, second];
    both.sort_by_key(|a| *a == Some(true));
    assert_eq!(
        both,
        [Some(false), Some(true)],
        "exactly one was signalled; the other waited out its deadline"
    );

    sync::cond_destroy(c);
}

/// A wait with nothing to wake it reports the timeout rather than hanging.
#[test]
fn a_wait_that_times_out_says_so() {
    let c = sync::create_cond("never");
    let started = std::time::Instant::now();
    assert_eq!(
        sync::cond_wait(c, Some(Duration::from_millis(80))),
        Some(false)
    );
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "it should have actually waited, not answered at once"
    );
    sync::cond_destroy(c);
}

/// A waiter with no deadline is woken by a signal from another thread.
#[test]
fn an_untimed_waiter_is_woken_by_a_signal() {
    let c = sync::create_cond("c");
    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = tx.send(sync::cond_wait(c, None));
    });

    assert!(
        rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "nothing has signalled yet"
    );
    assert_eq!(sync::cond_broadcast(c), Some(true));
    assert_eq!(rx.recv_timeout(PATIENCE), Ok(Some(true)));
    waiter.join().expect("the waiter finishes");

    sync::cond_destroy(c);
}

/// A condition-variable handle naming nothing answers "no such object".
#[test]
fn a_cond_handle_naming_nothing_answers_nothing() {
    let c = sync::create_cond("c");
    assert!(sync::cond_destroy(c));
    assert!(!sync::cond_destroy(c));

    assert_eq!(sync::cond_signal(c), None);
    assert_eq!(sync::cond_broadcast(c), None);
    assert_eq!(sync::cond_wait(c, Some(Duration::from_millis(1))), None);
    assert_eq!(sync::cond_name_of(c), None);
}

// --- read/write locks -------------------------------------------------------------------------

/// Readers do not wait for other readers.
///
/// **The whole point of the type**: a shared lock that queued readers behind each other
/// would be a mutex wearing another name, and would pass every test that only ever takes it
/// once.
#[test]
fn readers_do_not_wait_for_each_other() {
    let l = sync::create_rwlock("shared");
    assert_eq!(sync::rwlock_name_of(l).as_deref(), Some("shared"));

    assert_eq!(sync::rwlock_read(l, sync::Blocking::Forever), Some(true));
    assert_eq!(sync::rwlock_read(l, sync::Blocking::Forever), Some(true));
    assert_eq!(
        sync::rwlock_read(l, sync::Blocking::Never),
        Some(true),
        "three at once"
    );

    // A writer may not join them.
    assert_eq!(sync::rwlock_write(l, sync::Blocking::Never), Some(false));

    // Each reader has to leave before the writer may enter.
    assert_eq!(sync::rwlock_unlock(l), Some(true));
    assert_eq!(sync::rwlock_unlock(l), Some(true));
    assert_eq!(
        sync::rwlock_write(l, sync::Blocking::Never),
        Some(false),
        "one reader still holds it"
    );
    assert_eq!(sync::rwlock_unlock(l), Some(true));
    assert_eq!(sync::rwlock_write(l, sync::Blocking::Never), Some(true));

    assert_eq!(sync::rwlock_unlock(l), Some(true));
    assert!(sync::rwlock_destroy(l));
}

/// A writer excludes everybody, readers included.
#[test]
fn a_writer_excludes_readers_as_well_as_other_writers() {
    let l = sync::create_rwlock("exclusive");
    assert_eq!(sync::rwlock_write(l, sync::Blocking::Forever), Some(true));

    assert_eq!(sync::rwlock_read(l, sync::Blocking::Never), Some(false));
    assert_eq!(sync::rwlock_write(l, sync::Blocking::Never), Some(false));

    assert_eq!(sync::rwlock_unlock(l), Some(true));
    assert_eq!(
        sync::rwlock_read(l, sync::Blocking::Never),
        Some(true),
        "and now readers may"
    );

    sync::rwlock_unlock(l);
    sync::rwlock_destroy(l);
}

/// Releasing a lock nobody holds is reported rather than ignored.
///
/// It is a real bug in the guest, and silence would let it corrupt whatever the lock was
/// protecting - the release is one call for both kinds, so an unbalanced one is exactly the
/// mistake this shape invites.
#[test]
fn releasing_an_rwlock_nobody_holds_is_reported() {
    let l = sync::create_rwlock("l");
    assert_eq!(sync::rwlock_unlock(l), Some(false));

    assert_eq!(sync::rwlock_read(l, sync::Blocking::Forever), Some(true));
    assert_eq!(sync::rwlock_unlock(l), Some(true));
    assert_eq!(
        sync::rwlock_unlock(l),
        Some(false),
        "the second release has nothing to release"
    );

    sync::rwlock_destroy(l);
}

/// A blocked writer is woken when the last reader leaves.
#[test]
fn a_blocked_writer_is_woken_when_the_readers_leave() {
    let l = sync::create_rwlock("l");
    assert_eq!(sync::rwlock_read(l, sync::Blocking::Forever), Some(true));

    let (tx, rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let _ = tx.send(sync::rwlock_write(l, sync::Blocking::Forever));
    });

    assert!(
        rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "a reader still holds it"
    );
    assert_eq!(sync::rwlock_unlock(l), Some(true));
    assert_eq!(rx.recv_timeout(PATIENCE), Ok(Some(true)));
    writer.join().expect("the writer finishes");

    sync::rwlock_unlock(l);
    sync::rwlock_destroy(l);
}

/// A read/write lock handle naming nothing answers "no such object".
#[test]
fn an_rwlock_handle_naming_nothing_answers_nothing() {
    let l = sync::create_rwlock("l");
    assert!(sync::rwlock_destroy(l));
    assert!(!sync::rwlock_destroy(l));

    assert_eq!(sync::rwlock_read(l, sync::Blocking::Never), None);
    assert_eq!(sync::rwlock_write(l, sync::Blocking::Never), None);
    assert_eq!(sync::rwlock_unlock(l), None);
    assert_eq!(sync::rwlock_name_of(l), None);
}

// --- barriers ---------------------------------------------------------------------------------

/// A barrier of one releases on arrival, and says who released it.
#[test]
fn a_barrier_of_one_releases_immediately() {
    let b = sync::create_barrier(1, "solo");
    assert_eq!(sync::barrier_name_of(b).as_deref(), Some("solo"));
    assert_eq!(
        sync::barrier_wait(b),
        Some(true),
        "the arrival is the release"
    );
    assert_eq!(sync::barrier_wait(b), Some(true), "and again next round");
    assert!(sync::barrier_destroy(b));
}

/// A barrier asked for nobody still needs somebody.
///
/// `needed` is floored at one, because a barrier that releases before anyone arrives is not
/// a barrier - and a zero would make the arrival count never reach it.
#[test]
fn a_barrier_of_zero_is_treated_as_a_barrier_of_one() {
    let b = sync::create_barrier(0, "zero");
    assert_eq!(sync::barrier_wait(b), Some(true));
    sync::barrier_destroy(b);
}

/// Two threads meet at a barrier, and exactly one is told it did the releasing.
///
/// Run twice over the same barrier, which is what the round number exists for: without it a
/// fast thread re-entering would be counted into a round a slow one had not yet left, and
/// the second meeting would release early.
#[test]
fn two_threads_meet_at_a_barrier_and_can_meet_again() {
    let b = sync::create_barrier(2, "pair");

    for round in 0..2 {
        let (tx, rx) = std::sync::mpsc::channel();
        let other = {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(sync::barrier_wait(b));
            })
        };
        let mine = sync::barrier_wait(b);
        let theirs = rx.recv_timeout(PATIENCE).expect("both threads arrive");
        other.join().expect("the other thread finishes");

        let released = u8::from(mine == Some(true)) + u8::from(theirs == Some(true));
        assert_eq!(
            released, 1,
            "exactly one arrival releases a barrier, round {round}"
        );
    }

    assert!(sync::barrier_destroy(b));
}

/// A barrier handle naming nothing answers "no such object".
#[test]
fn a_barrier_handle_naming_nothing_answers_nothing() {
    let b = sync::create_barrier(1, "b");
    assert!(sync::barrier_destroy(b));
    assert!(!sync::barrier_destroy(b));

    assert_eq!(sync::barrier_wait(b), None);
    assert_eq!(sync::barrier_name_of(b), None);
}

// --- event flags -------------------------------------------------------------------------------

/// A poll distinguishes "no such flag" from "the pattern is not set".
///
/// **The reason the answer nests.** The first is a bad handle and the second is an ordinary
/// miss, and a guest branches differently on each - collapsing them would make a destroyed
/// flag look like a condition that has simply not happened yet, which a caller waits on
/// forever.
#[test]
fn a_missing_flag_and_an_unset_pattern_are_different_answers() {
    let e = sync::create_event_flag(0b0100, "state");

    assert_eq!(
        sync::event_flag_poll(e, 0b0100, false),
        Some(Some(0b0100)),
        "set: the flag exists and the pattern matched"
    );
    assert_eq!(
        sync::event_flag_poll(e, 0b1000, false),
        Some(None),
        "unset: the flag exists and the pattern did not match"
    );

    assert!(sync::event_flag_destroy(e));
    assert_eq!(
        sync::event_flag_poll(e, 0b0100, false),
        None,
        "gone: there is no flag to ask"
    );
}

/// `all` requires every requested bit; the default requires any of them.
#[test]
fn polling_for_all_bits_differs_from_polling_for_any() {
    let e = sync::create_event_flag(0b0101, "bits");

    assert_eq!(sync::event_flag_poll(e, 0b0111, false), Some(Some(0b0101)));
    assert_eq!(
        sync::event_flag_poll(e, 0b0111, true),
        Some(None),
        "bit two is missing, so not all of them are present"
    );
    assert_eq!(sync::event_flag_poll(e, 0b0101, true), Some(Some(0b0101)));

    // Asking for no bits at all cannot be satisfied by "all of them", which would otherwise
    // be vacuously true and wake every waiter on an empty pattern.
    assert_eq!(sync::event_flag_poll(e, 0, true), Some(None));
    assert_eq!(sync::event_flag_poll(e, 0, false), Some(None));

    sync::event_flag_destroy(e);
}

/// Setting adds bits; clearing keeps only the bits named.
///
/// **`clear` is a mask, not a subtraction** - it clears every bit *outside* the pattern,
/// which is the opposite of what the name suggests to a reader who has not checked.
#[test]
fn setting_adds_bits_and_clearing_keeps_only_those_named() {
    let e = sync::create_event_flag(0b0001, "e");

    assert_eq!(sync::event_flag_set(e, 0b0110), Some(true));
    assert_eq!(
        sync::event_flag_poll(e, 0b0111, true),
        Some(Some(0b0111)),
        "set is an OR, so the original bit survives"
    );

    assert_eq!(sync::event_flag_clear(e, 0b0010), Some(true));
    assert_eq!(
        sync::event_flag_poll(e, 0b0010, true),
        Some(Some(0b0010)),
        "only the named bit is left"
    );
    assert_eq!(sync::event_flag_poll(e, 0b0101, false), Some(None));

    // Clearing against nothing empties it entirely.
    assert_eq!(sync::event_flag_clear(e, 0), Some(true));
    assert_eq!(sync::event_flag_poll(e, u64::MAX, false), Some(None));

    sync::event_flag_destroy(e);
}

/// The full width of the word is usable, including the top bit.
///
/// A pattern held as anything narrower would lose these silently, and a guest waiting on a
/// high bit would wait forever.
#[test]
fn the_whole_word_is_usable_including_the_top_bit() {
    let top = 1_u64 << 63;
    let e = sync::create_event_flag(top, "wide");
    assert_eq!(sync::event_flag_poll(e, top, true), Some(Some(top)));

    assert_eq!(sync::event_flag_set(e, 1), Some(true));
    assert_eq!(sync::event_flag_poll(e, top | 1, true), Some(Some(top | 1)));

    sync::event_flag_destroy(e);
}

/// An event-flag handle naming nothing answers "no such object" from every call.
#[test]
fn an_event_flag_handle_naming_nothing_answers_nothing() {
    let e = sync::create_event_flag(0, "e");
    assert_eq!(sync::event_flag_name_of(e).as_deref(), Some("e"));
    assert!(sync::event_flag_destroy(e));
    assert!(!sync::event_flag_destroy(e));

    assert_eq!(sync::event_flag_set(e, 1), None);
    assert_eq!(sync::event_flag_clear(e, 1), None);
    assert_eq!(sync::event_flag_name_of(e), None);
}

// --- across the kinds --------------------------------------------------------------------------

/// Handles from different kinds of object are not interchangeable.
///
/// Every kind but the semaphore hands out an address-shaped `u64`, so a guest - or a bug
/// here - passing one to the wrong call must be told there is no such object rather than
/// finding a plausible one. They come from separate tables, and this is what proves it.
#[test]
fn a_handle_from_one_kind_of_object_means_nothing_to_another() {
    let mutex = sync::create(sync::Recursion::Forbidden, "m");
    let cond = sync::create_cond("c");
    let rwlock = sync::create_rwlock("l");
    let barrier = sync::create_barrier(1, "b");
    let flag = sync::create_event_flag(0, "e");

    assert_eq!(sync::cond_name_of(mutex), None);
    assert_eq!(sync::name_of(cond), None);
    assert_eq!(sync::rwlock_name_of(barrier), None);
    assert_eq!(sync::barrier_name_of(rwlock), None);
    assert_eq!(sync::event_flag_name_of(mutex), None);
    assert_eq!(sync::name_of(flag), None);

    sync::destroy(mutex);
    sync::cond_destroy(cond);
    sync::rwlock_destroy(rwlock);
    sync::barrier_destroy(barrier);
    sync::event_flag_destroy(flag);
}

// --- deadlines ---------------------------------------------------------------------------------
//
// The three primitives that gained a deadline this batch, tested for the property the
// deadline exists to give: a bounded total wait. Every one of these would pass against an
// implementation that waited forever, if it only asserted the return value - so each one
// measures the elapsed time as well.

/// A span short enough that a test waiting it out is not slow, long enough that the wait is
/// real rather than a scheduling accident.
const BRIEF: Duration = Duration::from_millis(80);

/// A deadline that has already passed is a wait of no time, **not a refusal**.
///
/// POSIX says the timeout of a `pthread_mutex_timedlock` is not consulted when the mutex can
/// be locked at once, so a caller that arrives late at a free lock still gets it. An
/// implementation that checked the clock first would fail this and would look correct
/// everywhere else.
#[test]
fn a_deadline_already_past_still_takes_something_that_is_free() {
    let m = sync::create(sync::Recursion::Forbidden, "late");
    let a_second_ago = std::time::Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("the host clock has been up longer than a second");
    let past = sync::Blocking::Until(a_second_ago);
    assert_eq!(
        sync::acquire(m, ALICE, past),
        Some(sync::Acquisition::Locked),
        "free is free, however late the caller is"
    );
    sync::unlock(m, ALICE);
    sync::destroy(m);
}

/// A timed acquisition of a held lock gives up, and gives up **when it said it would**.
#[test]
fn a_timed_acquisition_gives_up_at_its_deadline() {
    let m = sync::create(sync::Recursion::Forbidden, "held");
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );

    let started = std::time::Instant::now();
    let outcome = sync::acquire(m, BOB, sync::Blocking::Until(started + BRIEF));
    let waited = started.elapsed();

    assert_eq!(outcome, Some(sync::Acquisition::Busy), "it never came free");
    assert!(waited >= BRIEF, "and it did not give up early: {waited:?}");
    assert!(waited < PATIENCE, "nor wait past its deadline: {waited:?}");

    sync::unlock(m, ALICE);
    sync::destroy(m);
}

/// A lock released before the deadline is taken, not waited out.
#[test]
fn a_timed_acquisition_takes_a_lock_that_comes_free_in_time() {
    let m = sync::create(sync::Recursion::Forbidden, "soon");
    assert_eq!(
        sync::acquire(m, ALICE, sync::Blocking::Forever),
        Some(sync::Acquisition::Locked)
    );
    std::thread::spawn(move || {
        std::thread::sleep(BRIEF / 4);
        sync::unlock(m, ALICE);
    });

    assert_eq!(
        sync::acquire(
            m,
            BOB,
            sync::Blocking::Until(std::time::Instant::now() + PATIENCE)
        ),
        Some(sync::Acquisition::Locked),
        "released well inside the deadline"
    );
    sync::unlock(m, BOB);
    sync::destroy(m);
}

/// **Repeated wakes must not extend the wait**, which is the bug the deadline arithmetic
/// exists to prevent.
///
/// Handing the whole remaining span to each `wait_timeout` restarts the clock every time the
/// condition variable wakes a waiter, and this module's release notifies *all* of them - so
/// a writer waiting behind readers that keep arriving and leaving would be woken, find the
/// lock still taken, and start its timeout again from the top. It would never return.
///
/// The churn is deliberate: readers enter and leave continuously for longer than the
/// writer's deadline, so every one of those releases wakes the writer with nothing for it.
///
/// # Why the wait happens on its own thread
///
/// **The regression this guards against is an infinite wait**, so a test that simply called
/// `rwlock_write` and asserted afterwards would hang rather than fail - and a hanging test
/// takes the whole suite with it, reporting nothing about what broke. This was confirmed
/// the hard way: the arithmetic was reverted deliberately and the first version of this test
/// stopped responding instead of failing. So the wait runs on a thread with a channel, and
/// the deadline that matters is the one on the `recv`.
#[test]
fn wakes_that_bring_nothing_do_not_extend_a_deadline() {
    let l = sync::create_rwlock("churn");
    assert_eq!(
        sync::rwlock_read(l, sync::Blocking::Forever),
        Some(true),
        "one reader holds it throughout, so the writer can never proceed"
    );

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let churning = {
        let stop = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                // Each release notifies every waiter, so each turn is one pointless wake
                // for the writer below.
                sync::rwlock_read(l, sync::Blocking::Forever);
                sync::rwlock_unlock(l);
                std::thread::yield_now();
            }
        })
    };

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let outcome = sync::rwlock_write(l, sync::Blocking::Until(started + BRIEF));
        let _ = tx.send((outcome, started.elapsed()));
    });
    let answered = rx.recv_timeout(PATIENCE);

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    churning.join().expect("the churn thread should finish");
    sync::rwlock_unlock(l);

    let (outcome, waited) =
        answered.expect("the deadline must bound the total wait, not each turn of it - this hung");
    assert_eq!(outcome, Some(false), "a reader held it the whole time");
    assert!(waited >= BRIEF, "and it did not give up early: {waited:?}");

    sync::rwlock_destroy(l);
}

/// A timed semaphore take gives up, and one signalled in time does not.
/// **A timed wait must not report a timeout before its deadline.**
///
/// The invariant `a_timed_semaphore_take_gives_up_and_can_be_rescued` asserts once, and it
/// failed once - under the load of a whole-workspace run, never on its own. Once is a
/// coincidence to that test and a property to this one: fifty short waits, each of which must
/// have lasted at least as long as it was told to.
///
/// **The cause is not established.** The leading guess was rounding - `Condvar::wait_timeout`
/// is handed the span remaining until a fixed instant, and a host that rounds it *down* to its
/// timer granularity would report `timed_out` a fraction early. Fifty-eight attempts across
/// 5ms and 80ms deadlines did not reproduce it, so that guess is unsupported and this test is
/// an instrument rather than a regression test: it exercises the property fifty times a run
/// instead of once, so the next occurrence lands here with a turn number attached.
#[test]
fn a_timed_wait_never_gives_up_before_its_deadline() {
    const SHORT: Duration = Duration::from_millis(5);
    let s = sync::create_semaphore(0, 4, "never-signalled");
    for turn in 0..50 {
        let started = std::time::Instant::now();
        let outcome = sync::semaphore_wait(s, sync::Blocking::Until(started + SHORT));
        let waited = started.elapsed();
        assert_eq!(outcome, Some(false), "turn {turn}: nothing to take");
        assert!(
            waited >= SHORT,
            "turn {turn}: gave up after {waited:?}, which is short of the {SHORT:?} deadline"
        );
    }
}

#[test]
fn a_timed_semaphore_take_gives_up_and_can_be_rescued() {
    let s = sync::create_semaphore(0, 4, "empty");

    let started = std::time::Instant::now();
    assert_eq!(
        sync::semaphore_wait(s, sync::Blocking::Until(started + BRIEF)),
        Some(false),
        "nothing to take"
    );
    assert!(started.elapsed() >= BRIEF, "and it waited for it");

    std::thread::spawn(move || {
        std::thread::sleep(BRIEF / 4);
        sync::semaphore_signal(s, 1);
    });
    assert_eq!(
        sync::semaphore_wait(
            s,
            sync::Blocking::Until(std::time::Instant::now() + PATIENCE)
        ),
        Some(true),
        "signalled inside the deadline"
    );
    assert!(sync::semaphore_destroy(s));
}

/// The count a semaphore reports is the one it hands out.
#[test]
fn a_semaphore_reports_the_count_it_will_hand_out() {
    let s = sync::create_semaphore(3, 4, "counted");
    assert_eq!(sync::semaphore_value(s), Some(3));
    assert_eq!(sync::semaphore_wait(s, sync::Blocking::Never), Some(true));
    assert_eq!(sync::semaphore_value(s), Some(2), "one fewer after a take");
    assert_eq!(sync::semaphore_signal(s, 1), Some(true));
    assert_eq!(
        sync::semaphore_value(s),
        Some(3),
        "and one more after a give"
    );

    assert!(sync::semaphore_destroy(s));
    assert_eq!(
        sync::semaphore_value(s),
        None,
        "a handle naming nothing has no count, which is not a count of zero"
    );
}
