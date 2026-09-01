//! Keeping the guest thread pointer alive on a host that will not.
//!
//! Windows resets a user-set `fs` base to zero on the next context switch (D433), so the base
//! installed before a guest is entered does not survive the run. A guest `fs:`-relative access with
//! a zero base lands within ±2 GiB of zero - the whole reach of a signed 32-bit displacement - which
//! this process never maps, so such an access always *faults* rather than reading wrong data. That
//! makes the fault handler the one place that can reliably notice the base has reverted and put it
//! back before letting the instruction run again.
//!
//! This module holds the pointer to restore, per thread, and does the restoring. On a platform that
//! preserves the base (Linux with `FSGSBASE`), [`remember`] still records it and
//! [`restore_if_reverted`] simply never finds it zero, so the backstop costs nothing there.

use std::cell::Cell;

thread_local! {
    /// The guest thread pointer installed for this thread, or zero if none was.
    ///
    /// A `const` initialiser, so the fast thread-local path is used and the fault handler that reads
    /// this neither allocates nor lazily initialises anything - both of which are unsafe to do while
    /// a thread is mid-fault.
    static GUEST_TP: Cell<u64> = const { Cell::new(0) };
}

/// Records the thread pointer installed for the current thread, so the fault handler can restore it
/// if the host resets it out from under the guest.
pub fn remember(tp: u64) {
    GUEST_TP.set(tp);
}

/// If this thread has a guest thread pointer and its `fs` base has reverted to zero, re-install the
/// base and report that the faulting instruction should be retried.
///
/// `false` when there is nothing to restore (no pointer was installed for this thread) or when the
/// base is already valid - the second case being a *real* fault the caller must not swallow. The
/// distinction is the whole point: this restores a base the host quietly dropped, and stays out of
/// the way of every other fault.
#[must_use]
pub fn restore_if_reverted() -> bool {
    let tp = GUEST_TP.get();
    if tp == 0 {
        return false;
    }
    // The base at fault time. `Some(0)` is the reverted case; a non-zero base (or an unreadable one)
    // means the fault is not about a dropped base and belongs to the reporter.
    if orbistoun_abi::thread_pointer::current() != Some(0) {
        return false;
    }
    // SAFETY: restores the base this thread was already given - the block reserved for it, still
    // mapped for the life of the process. A single `wrfsbase`, affecting only this thread's base.
    unsafe { orbistoun_abi::thread_pointer::install(tp) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::{GUEST_TP, remember, restore_if_reverted};

    #[test]
    fn a_thread_with_no_pointer_has_nothing_to_restore() {
        // The ordinary case for every thread that is not running a guest with thread-locals: the
        // backstop must decline, so a real fault on such a thread still reaches the reporter.
        GUEST_TP.set(0);
        assert!(!restore_if_reverted());
    }

    #[test]
    fn remember_records_the_pointer_for_this_thread() {
        remember(0x6900_0000_1048);
        assert_eq!(GUEST_TP.get(), 0x6900_0000_1048);
        // A real restore needs a faulting `fs:` access to observe; what is unit-testable is that the
        // pointer is held per thread, which is what the fault handler reads.
        remember(0);
    }
}
