//! Stopping guest threads without freezing them where they stand.
//!
//! # Why not just suspend them
//!
//! The obvious implementation is the host's own suspend call against each guest thread, and
//! it is a trap. A thread stopped at an arbitrary instruction may be holding the C runtime's
//! heap lock, and the next allocation anywhere in the process then blocks forever - including
//! in the thread that would have issued the resume. The failure is a worker that is not
//! merely stopped but unrecoverable, and it happens only sometimes, which is worse.
//!
//! So threads are asked to stop rather than made to. Every guest call into this emulator
//! passes through one trampoline, and that is where a thread checks: it is in our code, it
//! holds no guest lock, and it is about to do nothing that cannot wait.
//!
//! # What that costs, stated rather than discovered
//!
//! **A guest thread that stops calling imports never parks.** A tight compute loop, a spin
//! on a flag, a thread waiting on something that will never arrive - none of them reach the
//! trampoline, so none of them stop.
//!
//! That is the same shape as the run limit, which exists because "a guest with every import
//! unimplemented can settle into a loop waiting for something that will never happen". The
//! difference is that this one is **counted**: [`parked`] against the number of threads that
//! were asked says how much of the guest actually stopped, so a session can report "two of
//! three threads parked" instead of implying all of them did (D344).

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Whether guest threads have been asked to stop.
static ASKED: AtomicBool = AtomicBool::new(false);

/// How many are stopped right now.
static PARKED: AtomicU32 = AtomicU32::new(0);

/// Asks every guest thread to stop at its next call into the emulator.
///
/// Returns immediately. Whether anything actually stopped is a separate question, which is
/// the point - see [`parked`].
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
/// **The honest half of the mechanism.** Compared against how many threads exist, it says
/// what fraction of the guest actually stopped rather than letting "suspended" imply all of
/// it did.
#[must_use]
pub fn parked() -> u32 {
    PARKED.load(Ordering::Acquire)
}

/// Stops here while threads are asked to stop.
///
/// Called from the trampoline, on a guest thread, before a shim runs.
///
/// **Allocates nothing and takes no lock**, which principle 9 requires of anything on this
/// path - and which is also the whole reason parking happens here rather than wherever a
/// thread happened to be. Yielding rather than spinning hot, because a parked title should
/// not keep a core busy doing nothing while somebody reads a menu.
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

    /// **A thread asked to stop does, and goes again when released - and one that was not
    /// asked passes straight through.**
    ///
    /// Driven from a second thread, because a parked one cannot release itself: the same
    /// arrangement as the emulator, where guest threads park and the reader thread carrying
    /// a shell request is what lets them go.
    ///
    /// One test rather than two. These are process-wide statics, so two tests touching them
    /// run concurrently under the harness and race - which is exactly what happened, and
    /// what makes the failure look like the mechanism being broken rather than the tests.
    #[test]
    fn a_thread_parks_until_it_is_released() {
        release();

        // Nothing asked, so this returns at once. The path that matters most in practice:
        // it runs on every single guest call, so it has to be a load and a branch.
        assert!(!asked());
        check();
        assert_eq!(parked(), 0);

        request();
        let guest = std::thread::spawn(|| {
            check();
        });

        // Wait for it to actually be parked rather than assuming it got there.
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
