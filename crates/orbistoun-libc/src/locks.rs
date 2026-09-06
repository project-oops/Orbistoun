//! The C runtime's internal recursive locks.
//!
//! # Why a real lock rather than a no-op
//!
//! The Dinkumware runtime this platform carries (D468) guards its own shared state with these:
//! a `FILE`'s buffer while one thread writes it, and a small set of numbered system locks
//! around things like the locale and `atexit`'s list. They are **recursive** - the same thread
//! may take one twice and must release it twice.
//!
//! A no-op passes every test a single-threaded guest can run and is a silent corruption the
//! moment two threads share a stream, which is exactly the failure this project has no cheap
//! way to notice. So these are real: an owner, a depth, and a condition variable to wait on.
//!
//! # Keyed by the argument, whatever it is
//!
//! `_Lockfilelock` takes a `FILE *` and `_Locksyslock` a small integer. Both are just a key
//! here, which keeps one implementation rather than two - and the two spaces cannot collide
//! because they are kept in separate tables.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};

/// Who holds a lock, and how deep.
#[derive(Debug, Clone, Copy)]
struct Held {
    owner: ThreadId,
    depth: u64,
}

/// One table of recursive locks, keyed by whatever the guest passed.
struct Locks {
    held: Mutex<BTreeMap<u64, Held>>,
    freed: Condvar,
}

impl Locks {
    const fn new() -> Self {
        Self {
            held: Mutex::new(BTreeMap::new()),
            freed: Condvar::new(),
        }
    }

    /// Takes the lock, waiting for another thread to finish with it.
    ///
    /// Re-entry by the owning thread increments the depth rather than deadlocking, which is
    /// what makes it recursive and is the whole reason the runtime can call a locked function
    /// from inside another one.
    fn lock(&self, key: u64) {
        let me = std::thread::current().id();
        let Ok(mut held) = self.held.lock() else {
            // A poisoned table means another thread panicked holding it. Blocking forever
            // would turn that into a hang with no explanation; proceeding is the lesser
            // wrong, and the panic itself is already reported.
            return;
        };
        loop {
            match held.get_mut(&key) {
                Some(entry) if entry.owner == me => {
                    entry.depth += 1;
                    return;
                }
                Some(_) => {
                    let Ok(next) = self.freed.wait(held) else {
                        return;
                    };
                    held = next;
                }
                None => {
                    held.insert(
                        key,
                        Held {
                            owner: me,
                            depth: 1,
                        },
                    );
                    return;
                }
            }
        }
    }

    /// Releases one level of the lock.
    ///
    /// A release by a thread that does not hold it is ignored rather than obeyed: honouring
    /// it would hand the lock to nobody while its real owner still believed it held one.
    fn unlock(&self, key: u64) {
        let me = std::thread::current().id();
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        let Some(entry) = held.get_mut(&key) else {
            return;
        };
        if entry.owner != me {
            return;
        }
        entry.depth -= 1;
        if entry.depth == 0 {
            held.remove(&key);
            self.freed.notify_all();
        }
    }
}

/// The stream locks, keyed by `FILE *`.
static FILE_LOCKS: Locks = Locks::new();
/// The system locks, keyed by the runtime's own small index.
static SYSTEM_LOCKS: Locks = Locks::new();

/// `_Lockfilelock(stream)` - takes a stream's lock, recursively.
fn lock_file(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    FILE_LOCKS.lock(args[0]);
    0
}

/// `_Unlockfilelock(stream)` - releases one level of it.
fn unlock_file(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    FILE_LOCKS.unlock(args[0]);
    0
}

/// `_Locksyslock(which)` - takes one of the runtime's numbered locks, recursively.
fn lock_system(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    SYSTEM_LOCKS.lock(args[0]);
    0
}

/// `_Unlocksyslock(which)` - releases one level of it.
fn unlock_system(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    SYSTEM_LOCKS.unlock(args[0]);
    0
}

/// Everything here, by symbol name.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("_Lockfilelock", lock_file),
        ("_Unlockfilelock", unlock_file),
        ("_Locksyslock", lock_system),
        ("_Unlocksyslock", unlock_system),
    ]
}

#[cfg(test)]
mod tests {
    use super::Locks;

    /// **Recursive, which is the whole point.** A lock taken twice by one thread must be
    /// released twice; a non-recursive implementation deadlocks on the second take, and the
    /// runtime does take them nested.
    #[test]
    fn one_thread_may_take_a_lock_twice_and_must_release_it_twice() {
        static IT: Locks = Locks::new();
        IT.lock(7);
        IT.lock(7);
        assert_eq!(
            IT.held.lock().unwrap().get(&7).map(|h| h.depth),
            Some(2),
            "the second take goes deeper rather than blocking"
        );
        IT.unlock(7);
        assert!(
            IT.held.lock().unwrap().contains_key(&7),
            "one release does not free a lock taken twice"
        );
        IT.unlock(7);
        assert!(
            !IT.held.lock().unwrap().contains_key(&7),
            "the matching release frees it"
        );
    }

    /// **The guard made to fail**: a release by a thread that does not hold the lock must be
    /// ignored. Obeying it would free a lock its real owner still believes it holds, which is
    /// the corruption a no-op implementation would cause on every call.
    #[test]
    fn a_release_by_a_thread_that_does_not_hold_it_is_ignored() {
        static IT: Locks = Locks::new();
        IT.lock(1);
        std::thread::scope(|scope| {
            scope.spawn(|| IT.unlock(1));
        });
        assert!(
            IT.held.lock().unwrap().contains_key(&1),
            "another thread's release must not free it"
        );
        IT.unlock(1);
    }

    /// A second thread waits rather than taking a held lock, and gets it once released.
    #[test]
    fn a_second_thread_waits_for_the_lock() {
        static IT: Locks = Locks::new();
        IT.lock(3);
        let taken = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                IT.lock(3);
                taken.store(true, std::sync::atomic::Ordering::SeqCst);
                IT.unlock(3);
            });
            // Give the waiter a chance to run and block, then release.
            std::thread::yield_now();
            assert!(
                !taken.load(std::sync::atomic::Ordering::SeqCst),
                "it must not have taken a lock this thread holds"
            );
            IT.unlock(3);
        });
        assert!(taken.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// The two tables are separate, so a `FILE *` that happens to equal a lock index does not
    /// collide with it.
    #[test]
    fn the_file_and_system_tables_are_separate() {
        super::FILE_LOCKS.lock(5);
        super::SYSTEM_LOCKS.lock(5);
        assert!(super::FILE_LOCKS.held.lock().unwrap().contains_key(&5));
        assert!(super::SYSTEM_LOCKS.held.lock().unwrap().contains_key(&5));
        super::FILE_LOCKS.unlock(5);
        super::SYSTEM_LOCKS.unlock(5);
    }
}
