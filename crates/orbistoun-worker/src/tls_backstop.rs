//! Keeping the guest thread pointer alive on a host that will not.
//!
//! Windows resets a user-set `fs` base to zero on a context switch. A guest `fs:`-relative access
//! with a zero base lands within 2 GiB of zero, which this process never maps, so it always faults
//! rather than reading wrong data, and the fault handler restores the base and retries. This module
//! holds the pointer to restore, per thread. On a platform that preserves the base, the backstop
//! never finds it zero and costs nothing.

use std::cell::Cell;

thread_local! {
    /// The guest thread pointer installed for this thread, or zero if none was.
    ///
    /// A `const` initialiser, so the fault handler that reads it neither allocates nor lazily
    /// initialises, neither of which is safe mid-fault.
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
/// `false` when no pointer was installed for this thread, or when the base is already valid, which
/// is a real fault the caller must not swallow.
#[must_use]
pub fn restore_if_reverted() -> bool {
    let tp = GUEST_TP.get();
    if tp == 0 {
        return false;
    }
    // A non-zero or unreadable base means the fault is not about a dropped base.
    if orbistoun_abi::thread_pointer::current() != Some(0) {
        return false;
    }
    // SAFETY: restores the base this thread was already given, a block that stays mapped for the
    // life of the process; it affects only this thread's base.
    unsafe { orbistoun_abi::thread_pointer::install(tp) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::{GUEST_TP, remember, restore_if_reverted};

    /// A thread with no guest pointer declines, so its real faults still reach the reporter.
    #[test]
    fn a_thread_with_no_pointer_has_nothing_to_restore() {
        GUEST_TP.set(0);
        assert!(!restore_if_reverted());
    }

    /// The pointer the fault handler reads is held per thread.
    #[test]
    fn remember_records_the_pointer_for_this_thread() {
        remember(0x6900_0000_1048);
        assert_eq!(GUEST_TP.get(), 0x6900_0000_1048);
        remember(0);
    }
}
