//! Software-interrupt dispatch: the kernel-entry boundary the guest reaches by `int n`.
//!
//! # Why this exists as a mechanism before a single handler
//!
//! orbistoun intercepts the guest at the *library* boundary (a relocation puts a stub where a
//! name was) and at the *syscall gadget* path (D376). A commercial title then reached the kernel a
//! third way - `int 0x41` - and orbistoun had no interrupt handling **at all**, so it caught the
//! trap as an unhandled host exception and stopped (worklog 603, 605). PPSA04263 runs on retail
//! hardware, so that is orbistoun's gap, not the guest's.
//!
//! Filling it has two halves that block on different things:
//!
//! - **The mechanism** - intercept the `int`, look up a handler for the vector, run it with the
//!   guest's register state, and resume the guest past the instruction. This depends on nothing
//!   external and is here.
//! - **The handlers** - what `int 0x41` actually *does* (which register carries the selector, what
//!   it reads and returns). That is a measurement on the device (obSCEne `REQ-...b3c2`), and until
//!   it lands, no handler is registered and every `int` still falls through to the honest
//!   "unimplemented kernel entry" fault.
//!
//! So this table is deliberately empty. Its value is that when the measurement arrives, servicing
//! `int 0x41` is one `install(0x41, handler)` call rather than an afternoon of plumbing - and the
//! plumbing is tested now, against a handler the tests register, rather than discovered to be wrong
//! the first time a real one is added.
//!
//! # Lock-free and allocation-free, because the caller is a fault handler
//!
//! `service` runs inside the vectored exception handler, where taking a lock or allocating can
//! deadlock the very process it is trying to resume (principle 9, and the reason `Line::hex` is
//! hand-rolled). So the table is a fixed array of atomic slots - one per vector, 256 of them - and
//! a handler is a bare `fn` pointer stored as its address. No map, no lock, no allocation.

use std::sync::atomic::{AtomicUsize, Ordering};

/// The guest register state a software-interrupt handler reads and may modify.
///
/// A handler services the interrupt by reading its arguments here (a syscall-gate `int` passes them
/// in registers, and often a selector in `rax`) and writing its result back - typically into `rax`,
/// the way a `sce*` function answers. [`rip`](Self::rip) is the point execution resumes at; the
/// dispatcher sets it past the `int` before the handler runs, so a handler that does nothing to it
/// simply returns to the instruction after the trap, and one that needs to jump can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "not bools; sixteen registers plus rip"
)]
pub struct InterruptFrame {
    /// The return-value register, and often the service selector on entry.
    pub rax: u64,
    /// General-purpose registers, in the usual order.
    pub rbx: u64,
    /// See [`Self::rbx`].
    pub rcx: u64,
    /// See [`Self::rbx`].
    pub rdx: u64,
    /// See [`Self::rbx`].
    pub rsi: u64,
    /// See [`Self::rbx`].
    pub rdi: u64,
    /// See [`Self::rbx`].
    pub rbp: u64,
    /// See [`Self::rbx`].
    pub rsp: u64,
    /// See [`Self::rbx`].
    pub r8: u64,
    /// See [`Self::rbx`].
    pub r9: u64,
    /// See [`Self::rbx`].
    pub r10: u64,
    /// See [`Self::rbx`].
    pub r11: u64,
    /// See [`Self::rbx`].
    pub r12: u64,
    /// See [`Self::rbx`].
    pub r13: u64,
    /// See [`Self::rbx`].
    pub r14: u64,
    /// See [`Self::rbx`].
    pub r15: u64,
    /// Where execution resumes. Preset past the `int` by the dispatcher; a handler may change it.
    pub rip: u64,
}

/// What a software-interrupt handler is: a function that services the trap by mutating the frame.
///
/// It answers in the frame - a value into `rax`, a jump by setting `rip` - the way a syscall gate
/// returns to its caller. It never allocates: it runs in the fault handler.
pub type InterruptHandler = fn(&mut InterruptFrame);

/// One handler slot per interrupt vector, holding the handler's address or zero for "none".
///
/// A `fn` pointer is one machine word, so it stores and loads atomically as a `usize`. Zero is not
/// a valid function address, which is what makes it the honest "no handler" sentinel.
static HANDLERS: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];

/// Registers `handler` for `int vector`. Called once, during setup, before the guest can trap.
pub fn install(vector: u8, handler: InterruptHandler) {
    HANDLERS[vector as usize].store(handler as usize, Ordering::Relaxed);
}

/// Removes any handler for `vector`, restoring the "unhandled" state.
///
/// Symmetric with [`install`], and the way a test leaves the table as it found it - the slots are
/// process-wide, so a test that installed one and did not clear it would leak into the next.
pub fn clear(vector: u8) {
    HANDLERS[vector as usize].store(0, Ordering::Relaxed);
}

/// Whether a vector has a handler, without running it - so a caller can decide before committing.
#[must_use]
pub fn has_handler(vector: u8) -> bool {
    HANDLERS[vector as usize].load(Ordering::Relaxed) != 0
}

/// Services `int vector` with the guest's `frame`, if a handler is registered.
///
/// Returns `true` when a handler ran and the guest may be resumed from `frame`, `false` when no
/// handler exists and the trap is still unhandled - the caller then reports it as the unimplemented
/// kernel entry it is, exactly as before this existed.
#[must_use]
pub fn service(vector: u8, frame: &mut InterruptFrame) -> bool {
    let raw = HANDLERS[vector as usize].load(Ordering::Relaxed);
    if raw == 0 {
        return false;
    }
    // SAFETY: `raw` is non-zero only because `install` stored a valid `InterruptHandler` fn
    // pointer there, and a `fn` pointer round-trips through `usize` unchanged. Transmuting it back
    // to the exact type it was stored from is sound.
    let handler: InterruptHandler = unsafe { std::mem::transmute::<usize, InterruptHandler>(raw) };
    handler(frame);
    true
}

#[cfg(test)]
mod tests {
    use super::{InterruptFrame, clear, has_handler, install, service};

    /// **A registered handler runs, reads the frame, and answers into it.**
    ///
    /// The whole mechanism in one test: install a handler for a vector, service it with a frame,
    /// and confirm the handler saw the arguments and its answer came back - which is what a real
    /// `int 0x41` handler will do once obSCEne says what it does. A high vector is used so no real
    /// registration could collide with it.
    #[test]
    fn a_registered_handler_services_the_interrupt_and_answers_in_the_frame() {
        fn double_rdi(frame: &mut InterruptFrame) {
            frame.rax = frame.rdi.wrapping_mul(2);
        }
        const VECTOR: u8 = 0xfe;
        clear(VECTOR);
        install(VECTOR, double_rdi);
        assert!(has_handler(VECTOR));

        let mut frame = InterruptFrame {
            rdi: 21,
            rip: 0x4000_0000_1002,
            ..InterruptFrame::default()
        };
        assert!(
            service(VECTOR, &mut frame),
            "a registered vector is serviced"
        );
        assert_eq!(frame.rax, 42, "the handler read rdi and answered in rax");
        clear(VECTOR);
    }

    /// **An unregistered vector is not serviced, and the frame is untouched.**
    ///
    /// The negative half is the load-bearing one: it is what keeps every `int` orbistoun cannot yet
    /// service falling through to the honest unimplemented-kernel-entry fault, rather than being
    /// silently swallowed. An empty table must answer "no", not "handled nothing".
    #[test]
    fn an_unregistered_vector_is_not_serviced() {
        const VECTOR: u8 = 0xfd;
        clear(VECTOR);
        let mut frame = InterruptFrame {
            rax: 0x1234,
            ..InterruptFrame::default()
        };
        assert!(
            !service(VECTOR, &mut frame),
            "no handler means not serviced"
        );
        assert_eq!(
            frame.rax, 0x1234,
            "an unserviced frame is left exactly as it was"
        );
        assert!(!has_handler(VECTOR));
    }

    /// **`int 0x41` in particular has no handler, and now the measurement says it never will.**
    ///
    /// Pinned as its own case because it is the vector this whole mechanism was built for. obSCEne
    /// has since measured it (`REQ-...b3c2`): a bare `int 0x41` from userspace on retail raises a
    /// signal and does not return - it is a fatal trap, not a callable service. So the honest state
    /// is not "unmeasured, so no handler yet" but "measured fatal, so there is nothing to register".
    /// The table must not answer for it, and a guest reaching it is diagnosed as a trap whose cause
    /// is upstream (orbistoun-report's `kernel_entry_finding`), not serviced here. The mechanism
    /// itself stays, for a *different* vector that measures as a real returning service.
    #[test]
    fn int_0x41_has_no_handler_because_it_is_measured_fatal() {
        assert!(
            !has_handler(0x41),
            "int 0x41 is measured fatal on retail (no return), so no handler may be registered for it"
        );
    }
}
