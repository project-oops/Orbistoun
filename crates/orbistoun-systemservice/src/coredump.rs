//! `libSceCoredump` - the crash-handler registration a title uses to be told about its own faults.
//!
//! **1 name, and it is answered honestly.** `sceCoredumpRegisterCoredumpHandler` accepts the title's
//! handler and reports success; the arity stays `6` (the trampoline's full capture, D504) because how
//! many arguments it really takes is not established, and a wrong arity only degrades a trace.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

/// Successful return, as the guest reads it.
const OK: u64 = 0;

guest_module! {
    "libSceCoredump" {
        "sceCoredumpRegisterCoredumpHandler" => 6,
    }
}

/// `sceCoredumpRegisterCoredumpHandler(handler, ...)` - accepts the title's crash handler.
///
/// **Accepted, on the same terms as [`crate::app_content::temporary_data_mount2`] and the affinity
/// setter (D523).** A title registers a handler to be told about its own faults; orbistoun catches guest
/// faults itself and does not coredump, so the handler is never invoked - but the registration succeeds,
/// which is what the caller tests. Unimplemented it answered the `0xf7ff0001` placeholder, a negative
/// code that tells the title its crash handler failed to register and may make it log or refuse; `0` is
/// the honest answer that the registration was taken.
///
/// **`known_by = assumed`:** the success return is the accept convention, not a hardware measurement, and
/// nothing here fabricates a handler id - the return is a status the caller tests against zero, not an
/// object it dereferences.
fn register_coredump_handler(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Implementations this module provides for `libSceCoredump`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[(
        "sceCoredumpRegisterCoredumpHandler",
        register_coredump_handler,
    )]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The handler registration is accepted with `0`, not the placeholder a stub would answer.
    ///
    /// Watched failing: a body that returned the `Unimplemented` placeholder would answer a negative
    /// code, which a caller testing the return reads as "registration failed" - the lie this replaces.
    #[test]
    fn a_coredump_handler_registration_is_accepted() {
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::register_coredump_handler(&args), super::OK);
    }
}
