//! `libSceIme` - the on-screen text-entry service.
//!
//! Every name here is declared and none is implemented, so the module is listed in
//! `SERVES_NOTHING`. Names are confirmed; arities are not. Each arity is `6`, the
//! trampoline's full capture, because a wrong arity only degrades a trace while a wrong
//! name is a shim nothing can reach (D504).

use orbistoun_hle::guest_module;

guest_module! {
    "libSceIme" {
        "sceImeKeyboardClose" => 6,
        "sceImeKeyboardGetInfo" => 6,
        "sceImeKeyboardGetResourceId" => 6,
        "sceImeKeyboardOpen" => 6,
        "sceImeUpdate" => 6,
    }
}
