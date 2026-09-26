//! The synchronisation primitives, exercised as a guest drives them.
//!
//! A synchronisation bug shows up as two guest threads in one critical section, far from its
//! cause, so these test behaviour across calls rather than single return values. The tables are
//! process-wide, but every `create_*` mints a fresh handle, so a test that only touches its own
//! objects is not disturbed by one running beside it. `ThreadHandle` is a caller-supplied `u64`,
//! so ownership is tested without spawning; tests that need blocking or waking spawn threads.

use orbistoun_kernel::sync;
use std::time::Duration;

/// Two thread identities that are not each other and not [`sync::NO_MUTEX`]'s zero.
const ALICE: u64 = 0x1001;
const BOB: u64 = 0x2002;

/// Long enough that a wake genuinely had to travel between threads, short enough that a
/// broken test fails rather than hangs.
const PATIENCE: Duration = Duration::from_secs(5);

/// A fresh mutex is a real handle with the name it was given.
///
/// The handle is a block this crate owns rather than a small integer, so a guest reading a
/// field through it finds memory instead of faulting.
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
    // And it is free again afterwards, which a leak would fail.
    assert_eq!(
        sync::acquire(m, BOB, sync::Blocking::Never),
        Some(sync::Acquisition::Locked)
    );
    assert_eq!(sync::unlock(m, BOB), Some(true));
    sync::destroy(m);
}

/// Re-locking a non-recursive mutex is refused without blocking.
///
/// The call runs on a thread with a deadline, so a regression fails instead of hanging.
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
/// A recursive lock that did not count would let another thread in one release early.
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
/// Releasing somebody else's lock would let two guest threads into one critical section.
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
    // And the real owner still holds it.
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
/// A normal lock the owner re-takes is busy, an error-checking one is a deadlock, and the
/// platform gives the two different codes. To another thread it is busy.
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

/// A blocked acquirer is woken when the lock is released, rather than left to a timeout.
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
/// Distinct from `Some(false)`, a real object refusing: collapsing them would make a destroyed
/// handle look like a busy lock, which a caller retries forever.
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

/// A semaphore hands out its initial count and then refuses.
#[test]
fn a_semaphore_hands_out_its_count_and_then_refuses() {
    let s = sync::create_semaphore(2, 8, "slots");
    assert_ne!(s, sync::NO_SEMAPHORE);
    assert_eq!(sync::semaphore_name_of(s).as_deref(), Some("slots"));

    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Never),
        Some(true)
    );
    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Never),
        Some(true)
    );
    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Never),
        Some(false),
        "the count is spent"
    );

    assert_eq!(sync::semaphore_signal(s, 1), Some(true));
    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Never),
        Some(true),
        "and returned"
    );
    assert!(sync::semaphore_destroy(s));
}

/// A handle is a counter, not a truncated pointer.
///
/// The handle is an `int`, and a host address truncated to four bytes can collide with another
/// semaphore's. Two created in a row differ by exactly one.
#[test]
fn semaphore_handles_are_counted_rather_than_derived_from_addresses() {
    let a = sync::create_semaphore(0, 1, "a");
    let b = sync::create_semaphore(0, 1, "b");
    assert_ne!(a, b);
    assert!(a > 0 && b > 0, "zero keeps meaning nothing here");
    sync::semaphore_destroy(a);
    sync::semaphore_destroy(b);
}

/// A signal past the ceiling is refused, not clamped, so a guest that lost count is told.
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
        assert_eq!(
            sync::semaphore_wait(s, 1, sync::Blocking::Never),
            Some(true)
        );
    }
    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Never),
        Some(false)
    );

    // An addition that would overflow the counter is refused before the ceiling comparison
    // rather than wrapping to a small number that passes it.
    assert_eq!(sync::semaphore_signal(s, u32::MAX), Some(false));
    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Never),
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
        let _ = tx.send(sync::semaphore_wait(s, 1, sync::Blocking::Forever));
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

    assert_eq!(sync::semaphore_wait(s, 1, sync::Blocking::Never), None);
    assert_eq!(sync::semaphore_signal(s, 1), None);
    assert_eq!(sync::semaphore_name_of(s), None);
    assert_eq!(
        sync::semaphore_wait(sync::NO_SEMAPHORE, 1, sync::Blocking::Never),
        None
    );
}

/// A signal arriving before anybody waits is remembered.
///
/// Signals are counted rather than relying on the host notify alone, which a waiter arriving
/// later would miss.
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
    // And it was consumed.
    assert_eq!(
        sync::cond_wait(c, Some(Duration::from_millis(50))),
        Some(false),
        "one signal is one wake"
    );

    assert!(sync::cond_destroy(c));
}

/// One signal wakes one waiter, however many the notify reaches.
///
/// `cond_broadcast` calls the host's `notify_all`, so both waiters wake; the signal count
/// decides that only one leaves the wait. This stands in for a spurious wake, which cannot be
/// provoked on demand.
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

/// Readers do not wait for other readers.
///
/// A shared lock that queued readers behind each other would be a mutex under another name.
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
/// The release is one call for both kinds, so an unbalanced release is an easy guest bug.
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
/// `needed` is floored at one: a barrier that releases before anyone arrives is not a barrier.
#[test]
fn a_barrier_of_zero_is_treated_as_a_barrier_of_one() {
    let b = sync::create_barrier(0, "zero");
    assert_eq!(sync::barrier_wait(b), Some(true));
    sync::barrier_destroy(b);
}

/// Two threads meet at a barrier, and exactly one is told it did the releasing.
///
/// Run twice over the same barrier: the round number keeps a fast thread re-entering from being
/// counted into a round a slow one has not left.
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

/// A poll distinguishes "no such flag" from "the pattern is not set".
///
/// The first is a bad handle and the second an ordinary miss; collapsing them would make a
/// destroyed flag look like a condition a caller waits on forever.
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

    // No bits at all cannot satisfy "all of them", which would otherwise be vacuously true.
    assert_eq!(sync::event_flag_poll(e, 0, true), Some(None));
    assert_eq!(sync::event_flag_poll(e, 0, false), Some(None));

    sync::event_flag_destroy(e);
}

/// Setting adds bits; clearing keeps only the bits named.
///
/// `clear` is a mask: it clears every bit outside the pattern.
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

/// Handles from different kinds of object are not interchangeable.
///
/// Most kinds hand out an address-shaped `u64`; they come from separate tables, so a handle
/// passed to the wrong call finds no such object.
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

// Deadlines: each test measures elapsed time as well as the answer, because an
// implementation that waited forever would pass on the return value alone.

/// A span short enough that a test waiting it out is not slow, long enough that the wait is
/// real rather than a scheduling accident.
const BRIEF: Duration = Duration::from_millis(80);

/// A deadline that has already passed is a wait of no time, not a refusal.
///
/// POSIX does not consult the timeout of `pthread_mutex_timedlock` when the mutex can be
/// locked at once, so a late caller at a free lock still gets it.
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

/// A timed acquisition of a held lock gives up when its deadline passes.
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

/// Repeated wakes do not extend the wait.
///
/// Every release notifies all waiters, so a writer behind churning readers is woken many times
/// with nothing for it; handing each `wait_timeout` the whole span again would restart the
/// clock and never return. The wait runs on its own thread with a deadline on the `recv`, so
/// the regression fails instead of hanging.
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
                // Each release notifies every waiter, so each turn is one empty wake for the writer.
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

/// A timed wait never reports a timeout before its deadline.
///
/// Fifty short waits, each of which must last at least as long as it was told to; a host that
/// rounds the remaining span down to its timer granularity would fail it.
#[test]
fn a_timed_wait_never_gives_up_before_its_deadline() {
    const SHORT: Duration = Duration::from_millis(5);
    let s = sync::create_semaphore(0, 4, "never-signalled");
    for turn in 0..50 {
        let started = std::time::Instant::now();
        let outcome = sync::semaphore_wait(s, 1, sync::Blocking::Until(started + SHORT));
        let waited = started.elapsed();
        assert_eq!(outcome, Some(false), "turn {turn}: nothing to take");
        assert!(
            waited >= SHORT,
            "turn {turn}: gave up after {waited:?}, which is short of the {SHORT:?} deadline"
        );
    }
}

/// A timed semaphore take gives up, and one signalled in time does not.
#[test]
fn a_timed_semaphore_take_gives_up_and_can_be_rescued() {
    let s = sync::create_semaphore(0, 4, "empty");

    let started = std::time::Instant::now();
    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Until(started + BRIEF)),
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
            1,
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
    assert_eq!(
        sync::semaphore_wait(s, 1, sync::Blocking::Never),
        Some(true)
    );
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
