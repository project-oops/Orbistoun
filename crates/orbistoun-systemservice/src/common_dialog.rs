//! `libSceCommonDialog` - the shared initialisation every system dialog goes through.
//!
//! Arity is `6`, the trampoline's full capture, not a claim about how many arguments the function
//! takes; the real function takes none.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceCommonDialog" {
        "sceCommonDialogInitialize" => 6,
    }
}

/// Successful initialisation, as the guest reads it.
const OK: u64 = 0;

/// `sceCommonDialogInitialize()` - starts the common-dialog subsystem. Answers `0`.
///
/// Guest-observed: titles call it at startup, test the sign of the return and exit on a negative
/// one. The library is not linked into a title process by default, so its return cannot be
/// measured in that context. There is no out-parameter; the guest consumes only the sign.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Implementations this module provides for `libSceCommonDialog`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceCommonDialogInitialize", initialize)]
}
