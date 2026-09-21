//! `libSceErrorDialog` - the system error dialog.
//!
//! **1 name, and its init is served.** `sceErrorDialogInitialize` answers the honest init `OK`
//! (handler in `lib.rs`, worklog 756); the name comes from the recorded corpus (1).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! No longer in `SERVES_NOTHING`: setting the subsystem up succeeds honestly, the same reasoning
//! `libSceAudioOut`'s init retired on.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceErrorDialog" {
        "sceErrorDialogInitialize" => 6,
    }
}
