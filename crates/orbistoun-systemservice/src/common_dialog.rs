//! `libSceCommonDialog` - the shared initialisation every system dialog goes through.
//!
//! **1 name, from PPSA02664's own import table.** Arity is `6`, the trampoline's full capture,
//! which is not a claim about how many arguments it takes - the reasoning is `orbistoun-gpu`'s
//! `agc` module in full (D504); the short form is that a wrong arity only degrades a trace while
//! a wrong name is a shim nothing can reach. The real function takes none (`void`).

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
/// **Guest-observed, and it is the ceiling of the available evidence.** Three retail Unity titles
/// (PPSA02664, PPSA03416, PPSA25872) call this early, check the sign of the return, and stop dead
/// on a negative one - PPSA02664 prints `sceCommonDialogInitialize() failed` and calls `exit`
/// (D670). All three ship and run on a console, so it succeeds there. Answering `0` lets PPSA02664
/// proceed 123 further distinct imports and 402,745 further calls of coherent engine setup (it
/// names its own work in debug markers - "Clear", "Disable MSAA/EQAA") before it reaches the GPU
/// command-buffer layer, which is the second, different observation that the return is right rather
/// than merely unblocking (D227).
///
/// **It cannot be measured, which is why guest-observed is where this stops.** obSCEne's a3f7
/// established that `libSceCommonDialog` is not linked into a title process by default: the symbol
/// is `absent|shared` and unreachable without `sceSysmoduleLoadModule(SCE_SYSMODULE_MESSAGE_DIALOG)`
/// first, so its return cannot be read on hardware in the context that matters. The measurement path
/// is closed; the guest oracle plus three shipping titles is the strongest evidence obtainable.
///
/// No out-parameter: unlike the Earthion mapper gate (D677), the guest consumes only the sign of the
/// return, so `0` is one honest value rather than a value plus an unmeasured structure. This is the
/// same shape as [`super::sysmodule_load_module`] - an init call answered `0` because that is the
/// honest answer and the guest needs it, not a stub dressed as knowledge.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Implementations this module provides for `libSceCommonDialog`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceCommonDialogInitialize", initialize)]
}
