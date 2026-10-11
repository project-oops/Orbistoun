//! `libSceNpGameIntent` - the intents a title is launched or resumed with (an activity, a join).
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceNpGameIntentInitialize` is implemented,
//! as measured.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceNpGameIntent" {
        // (param)
        "sceNpGameIntentInitialize" => 1,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceNpGameIntentInitialize", initialize)]
}

/// `sceNpGameIntentInitialize(param)`: with `param` a block whose first u64 is its size, 0x28, and
/// the rest zero, answers 0 and leaves the block as it was (REQ-np02, two sweeps:
/// `20261011-083754-eboot.obs.log` 2933-2960 and `20261011-1791715554-eboot.obs.log` 2987-3014,
/// after `sceSysmoduleLoadModule(0x011c)`). A null block is unmeasured and answers the placeholder.
fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    0
}
