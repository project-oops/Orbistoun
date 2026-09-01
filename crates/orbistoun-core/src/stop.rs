//! Stopping, when the guest asks to.
//!
//! # Why this needs a hook rather than a call
//!
//! A guest that calls `abort` or `exit` has decided to stop. The subsystem crate that
//! receives the call is the wrong place to decide *how* - it does not know whether a trace
//! is being written, where it goes, or what a shim expects to see. The worker knows all of
//! that and sits above the subsystem crates, so it cannot be called downwards.
//!
//! So the worker installs a handler here and the subsystems call it, which is the ordinary
//! way to invert a dependency and the only one that keeps the spine intact (principle 6).
//!
//! # What this fixes, and it was reporting the opposite of the truth
//!
//! `abort` is declared `noreturn`, so a compiler emits a trap immediately after the call -
//! that code is unreachable by contract. An unimplemented `abort` **returns**, execution
//! falls into the trap, and the run reports `illegal instruction`.
//!
//! Two titles were reporting exactly that. The guest had not executed anything invalid; it
//! had deliberately given up, and the emulator turned a clear statement of intent into a
//! confusing machine fault at an address that meant nothing (D177).

use std::sync::OnceLock;

/// Why the guest stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// `abort` - the guest hit a condition it refuses to continue past.
    Aborted,
    /// `exit` - an ordinary, deliberate end.
    Exited,
}

impl StopReason {
    /// How to describe it in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Aborted => "the guest called abort",
            Self::Exited => "the guest called exit",
        }
    }
}

/// What to do when a guest asks to stop. Installed by whoever owns the run.
type Handler = fn(StopReason, u64) -> !;

/// The installed handler.
static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Installs the handler. The first caller wins; later ones are ignored.
///
/// Called once, during setup, by the process that owns the trace - so that a guest
/// stopping is recorded rather than merely happening.
pub fn on_guest_stop(handler: Handler) {
    let _ = HANDLER.set(handler);
}

/// Stops, because the guest asked to. Never returns.
///
/// `code` is whatever the guest passed, which is an exit status for `exit` and meaningless
/// for `abort` - carried through rather than interpreted.
///
/// **Without a handler this still does not return.** A subsystem calling this has already
/// established that the guest will not continue, and returning would put execution into
/// the unreachable trap the compiler placed after the call - which is precisely the
/// failure this exists to remove.
pub fn stop(reason: StopReason, code: u64) -> ! {
    if let Some(handler) = HANDLER.get() {
        handler(reason, code);
    }
    // No handler: nothing is recording, so there is nothing to flush. Said plainly on the
    // error stream first, because a process that vanishes silently is indistinguishable
    // from one that crashed.
    eprintln!("orbistoun: {} ({code:#x})", reason.label());
    std::process::exit(EXIT_GUEST_STOPPED);
}

/// Exit status for a run the guest ended itself.
///
/// Distinct from a crash and from the time limit, so a shim can tell "it gave up" from "it
/// died" without parsing anything.
pub const EXIT_GUEST_STOPPED: i32 = 0x0B0F;

#[cfg(test)]
mod tests {
    use super::StopReason;

    #[test]
    fn every_reason_says_what_happened_in_words() {
        // These go straight into a report a person reads. "Aborted" is a state; "the guest
        // called abort" is a sentence, and the difference matters when the alternative
        // report was `illegal instruction` at a meaningless address.
        assert!(StopReason::Aborted.label().contains("abort"));
        assert!(StopReason::Exited.label().contains("exit"));
    }

    #[test]
    fn the_stopped_status_is_not_the_ordinary_success() {
        // A shim distinguishes "gave up" from "ran to completion" and from "was killed",
        // and a status that collided with one of those would erase the distinction.
        assert_ne!(super::EXIT_GUEST_STOPPED, 0);
    }
}
