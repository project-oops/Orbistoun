//! `libSceCommonDialog` - the shared initialisation every system dialog goes through.
//!
//! **1 names, declared and not implemented.** They come from PPSA02664's own import table (1).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceCommonDialog" {
        "sceCommonDialogInitialize" => 6,
    }
}
