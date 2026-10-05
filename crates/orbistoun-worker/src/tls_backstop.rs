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

/// How many times one fault re-installs a base the host keeps dropping before declining it.
///
/// The host can switch the thread out between the write and its read-back, which drops the base
/// again; that is the very reset being repaired, not a refusal. Each attempt is two instructions,
/// so a run of them losing the race every time means the base is not kept at all.
const RESTORE_ATTEMPTS: usize = 16;

/// If this thread has a guest thread pointer and its `fs` base has reverted to zero, re-install the
/// base and report that the faulting instruction should be retried.
///
/// `false` when no pointer was installed for this thread, or when the base is already valid, which
/// is a real fault the caller must not swallow.
#[must_use]
pub fn restore_if_reverted() -> bool {
    restore(
        GUEST_TP.get(),
        orbistoun_abi::thread_pointer::current(),
        |tp| {
            // SAFETY: restores the base this thread was already given, a block that stays mapped
            // for the life of the process; it affects only this thread's base.
            unsafe { orbistoun_abi::thread_pointer::install(tp) }
        },
    )
}

/// [`restore_if_reverted`]'s decision: `tp` the thread's guest pointer, `base` its `fs` base when
/// the fault was taken, `install` the write. A base not kept is written again, since the host
/// dropping it between the write and the read-back is the reset itself.
fn restore(
    tp: u64,
    base: Option<u64>,
    mut install: impl FnMut(u64) -> Result<(), orbistoun_abi::thread_pointer::Unsupported>,
) -> bool {
    // No pointer, or a non-zero or unreadable base: the fault is not about a dropped base.
    if tp == 0 || base != Some(0) {
        return false;
    }
    for _ in 0..RESTORE_ATTEMPTS {
        match install(tp) {
            Ok(()) => return true,
            Err(orbistoun_abi::thread_pointer::Unsupported::NotRetained) => {}
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{GUEST_TP, remember, restore, restore_if_reverted};
    use orbistoun_abi::thread_pointer::Unsupported;

    /// A base the host drops again between the write and the read-back is written again, not
    /// declined: CRFT00001 and STKT00001 reported `fs:[0]` reads of zero when a switch landed
    /// there. Only a base that is never kept, or a write that cannot happen, is declined.
    #[test]
    fn a_base_dropped_again_is_written_again() {
        let mut dropped = 2;
        let flaky = |_| {
            if dropped > 0 {
                dropped -= 1;
                Err(Unsupported::NotRetained)
            } else {
                Ok(())
            }
        };
        assert!(restore(0x6900_0000_2130, Some(0), flaky));
        assert!(!restore(0x6900_0000_2130, Some(0), |_| Err(
            Unsupported::NotRetained
        )));
        assert!(!restore(0x6900_0000_2130, Some(0), |_| Err(
            Unsupported::NoProcessorSupport
        )));
        assert!(
            !restore(0x6900_0000_2130, Some(0x1000), |_| Ok(())),
            "a live base"
        );
        assert!(!restore(0, Some(0), |_| Ok(())), "no pointer");
    }

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
