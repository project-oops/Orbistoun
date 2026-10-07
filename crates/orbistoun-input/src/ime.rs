//! `libSceIme` - the on-screen text-entry service.
//!
//! Every name here is declared; only `sceImeUpdate` is implemented. Names are confirmed;
//! arities are not. Each arity is `6`, the trampoline's full capture, because a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach (D504).

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;

/// What `sceImeUpdate` answers with no keyboard open: `0x80bc0002`, measured with every argument
/// a poisoned buffer and with every argument zero, nothing written (obSCEne census,
/// `200-census/libSceIme/sceImeUpdate`, sweep 20261007-202010). orbistoun opens no keyboard.
const NO_KEYBOARD_OPEN: u64 = 0x80bc_0002;

/// `sceImeUpdate(handler)` - delivers pending keyboard events to the handler. With no keyboard
/// open there are none, and the call answers as the console answers then.
pub(crate) fn update(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NO_KEYBOARD_OPEN
}

guest_module! {
    "libSceIme" {
        "sceImeKeyboardClose" => 6,
        "sceImeKeyboardGetInfo" => 6,
        "sceImeKeyboardGetResourceId" => 6,
        "sceImeKeyboardOpen" => 6,
        "sceImeUpdate" => 6,
    }
}
