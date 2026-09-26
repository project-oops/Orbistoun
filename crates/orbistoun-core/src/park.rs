//! Stopping guest threads at a safe point instead of suspending them.
//!
//! A thread suspended at an arbitrary instruction may hold the C runtime's heap lock, and the
//! next allocation anywhere then blocks forever, including in the thread that would resume it.
//! So threads are asked to stop, and check at the one trampoline every guest call passes
//! through, where they hold no guest lock. A thread that stops calling imports never parks, so
//! [`parked`] counts how many did (D344).

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Whether guest threads have been asked to stop.
static ASKED: AtomicBool = AtomicBool::new(false);

/// How many are stopped right now.
static PARKED: AtomicU32 = AtomicU32::new(0);

/// Asks every guest thread to stop at its next call into the emulator.
///
/// Returns immediately; [`parked`] says whether anything stopped.
pub fn request() {
    ASKED.store(true, Ordering::Release);
}

/// Lets parked threads go.
pub fn release() {
    ASKED.store(false, Ordering::Release);
}

/// Whether threads are currently asked to stop.
#[must_use]
pub fn asked() -> bool {
    ASKED.load(Ordering::Acquire)
}

/// How many guest threads are stopped right now.
///
/// Compared against the number of threads, it says what fraction of the guest stopped.
#[must_use]
pub fn parked() -> u32 {
    PARKED.load(Ordering::Acquire)
}

/// Stops here while threads are asked to stop.
///
/// Called from the trampoline, on a guest thread, before a shim runs. Allocates nothing and
/// takes no lock (D344); yields rather than spinning so a parked title does not hold a core.
pub fn check() {
    if !asked() {
        return;
    }
    PARKED.fetch_add(1, Ordering::AcqRel);
    while asked() {
        std::thread::yield_now();
    }
    PARKED.fetch_sub(1, Ordering::AcqRel);
}

#[cfg(test)]
mod tests {
    use super::{asked, check, parked, release, request};

    /// A thread asked to stop parks and continues when released; one not asked passes through.
    ///
    /// Driven from a second thread, because a parked one cannot release itself. One test, because
    /// the statics are process-wide and two tests touching them would race.
    #[test]
    fn a_thread_parks_until_it_is_released() {
        release();

        // Nothing asked, so this returns at once: it runs on every guest call, so it is a load and a
        // branch.
        assert!(!asked());
        check();
        assert_eq!(parked(), 0);

        request();
        let guest = std::thread::spawn(|| {
            check();
        });

        // Wait for it to be parked rather than assuming it got there.
        let mut spins = 0;
        while parked() == 0 {
            std::thread::yield_now();
            spins += 1;
            assert!(spins < 5_000_000, "the thread never parked");
        }

        release();
        guest
            .join()
            .expect("the parked thread resumes and finishes");
        assert_eq!(parked(), 0, "and it is no longer counted as stopped");
    }
}
