//! `libSceCoredump` - the crash-handler registration a title uses to be told about its own faults.
//!
//! Arity is `6`, the trampoline's full capture: how many arguments the function takes is not
//! established, and a wrong arity only degrades a trace.

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
/// orbistoun catches guest faults itself and never invokes the handler, but the registration
/// succeeds, which is what the caller tests. The success return is an assumed accept
/// convention, not a measurement; no handler id is written.
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

    /// The handler registration is accepted with `0`, not the negative placeholder a stub answers.
    #[test]
    fn a_coredump_handler_registration_is_accepted() {
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::register_coredump_handler(&args), super::OK);
    }
}
