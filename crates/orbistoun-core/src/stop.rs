//! Stopping, when the guest asks to.
//!
//! A guest that calls `abort` or `exit` has decided to stop. The worker knows how to end a
//! run (trace, destination, what a shim expects) and sits above the subsystem crates, so it
//! installs a handler here and the subsystems call it. `abort` is `noreturn`, so a compiler
//! places a trap after the call; an `abort` that returned would run into it and be reported
//! as an illegal instruction instead of a deliberate stop (D177).

use std::sync::OnceLock;

/// Why the guest stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// `abort` - the guest hit a condition it refuses to continue past.
    Aborted,
    /// `exit` - an ordinary, deliberate end.
    Exited,
    /// A signal was raised on a thread and nothing was installed to handle it.
    ///
    /// A `sceKernelRaiseException` for a signal with no handler does not return: the process takes
    /// the signal and dies (obSCEne's `030-thread/exception-handler` check). The `code` is the
    /// signal number.
    Signalled,
}

impl StopReason {
    /// How to describe it in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Aborted => "the guest called abort",
            Self::Exited => "the guest called exit",
            Self::Signalled => "the guest raised a signal nothing was installed to handle",
        }
    }
}

/// What to do when a guest asks to stop. Installed by whoever owns the run.
type Handler = fn(StopReason, u64) -> !;

/// The installed handler.
static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Installs the handler. The first caller wins; later ones are ignored.
///
/// Called once during setup by the process that owns the trace, so a guest stop is recorded.
pub fn on_guest_stop(handler: Handler) {
    let _ = HANDLER.set(handler);
}

/// Stops, because the guest asked to. Never returns.
///
/// `code` is whatever the guest passed: an exit status for `exit`, meaningless for `abort`.
/// Without a handler this still does not return, since returning would run into the trap the
/// compiler placed after the call.
pub fn stop(reason: StopReason, code: u64) -> ! {
    if let Some(handler) = HANDLER.get() {
        handler(reason, code);
    }
    // No handler: nothing is recording, so there is nothing to flush. Logged first, because a
    // process that vanishes silently is indistinguishable from one that crashed.
    tracing::error!("{} ({code:#x})", reason.label());
    std::process::exit(EXIT_GUEST_STOPPED);
}

/// Exit status for a run the guest ended itself.
///
/// Distinct from a crash and from the time limit, so a shim can tell "it gave up" from "it died".
pub const EXIT_GUEST_STOPPED: i32 = 0x0B0F;

#[cfg(test)]
mod tests {
    use super::StopReason;

    /// Every stop reason's label names what the guest did in words.
    #[test]
    fn every_reason_says_what_happened_in_words() {
        // Labels go straight into a report a person reads, so they name the call the guest made.
        assert!(StopReason::Aborted.label().contains("abort"));
        assert!(StopReason::Exited.label().contains("exit"));
    }

    /// The guest-stopped exit status differs from ordinary success.
    #[test]
    fn the_stopped_status_is_not_the_ordinary_success() {
        // A status that collided with success or with a kill would erase the distinction.
        assert_ne!(super::EXIT_GUEST_STOPPED, 0);
    }
}
