//! Software-interrupt dispatch for the guest's `int n` kernel entries.
//!
//! The guest reaches the kernel through library stubs, syscall gadgets (D376) and `int n`. This
//! module is the mechanism for the third: a handler per vector, run with the guest's register
//! state, resuming past the instruction. A vector with no handler falls through to the
//! unimplemented-kernel-entry fault.
//!
//! `service` runs inside the vectored exception handler, where a lock or an allocation can
//! deadlock the process, so the table is a fixed array of 256 atomic slots holding `fn`
//! pointers.

use std::sync::atomic::{AtomicUsize, Ordering};

/// The guest register state a software-interrupt handler reads and may modify.
///
/// A handler reads its arguments here (often a selector in `rax`) and writes its result back,
/// typically into `rax`. [`rip`](Self::rip) is where execution resumes; the dispatcher sets it
/// past the `int` before the handler runs, and a handler may change it to jump.
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

/// A software-interrupt handler: services the trap by mutating the frame.
///
/// It answers in the frame, a value in `rax` or a jump through `rip`. It never allocates, because
/// it runs in the fault handler.
pub type InterruptHandler = fn(&mut InterruptFrame);

/// One handler slot per interrupt vector, holding the handler's address or zero for none.
///
/// A `fn` pointer is one machine word and is never zero, so it stores atomically as a `usize`
/// and zero is the empty sentinel.
static HANDLERS: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];

/// Registers `handler` for `int vector`. Called once, during setup, before the guest can trap.
pub fn install(vector: u8, handler: InterruptHandler) {
    HANDLERS[vector as usize].store(handler as usize, Ordering::Relaxed);
}

/// Removes any handler for `vector`, restoring the unhandled state.
///
/// The slots are process-wide, so a test clears what it installed.
pub fn clear(vector: u8) {
    HANDLERS[vector as usize].store(0, Ordering::Relaxed);
}

/// Whether a vector has a handler, without running it.
#[must_use]
pub fn has_handler(vector: u8) -> bool {
    HANDLERS[vector as usize].load(Ordering::Relaxed) != 0
}

/// Services `int vector` with the guest's `frame`, if a handler is registered.
///
/// Returns `true` when a handler ran and the guest may resume from `frame`, `false` when no
/// handler exists and the caller reports an unimplemented kernel entry.
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

    /// A registered handler runs, reads the frame's arguments and answers into it.
    ///
    /// A high vector keeps the test clear of any real registration.
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

    /// An unregistered vector is not serviced and its frame is untouched, so the trap reaches the
    /// unimplemented-kernel-entry fault rather than being swallowed.
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

    /// `int 0x41` has no handler.
    ///
    /// On the hardware a bare `int 0x41` from userspace raises a signal and does not return, so there
    /// is nothing to service; a guest reaching it is diagnosed by `kernel_entry_finding` in
    /// `orbistoun-report`.
    #[test]
    fn int_0x41_has_no_handler_because_it_is_measured_fatal() {
        assert!(
            !has_handler(0x41),
            "int 0x41 is measured fatal on retail (no return), so no handler may be registered for it"
        );
    }
}
