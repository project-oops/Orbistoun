//! `libSceMsgDialog.native` - the system message dialog.
//!
//! The names are declared and none is implemented, so the module is listed in `SERVES_NOTHING`.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceMsgDialog.native" {
        "sceMsgDialogInitialize" => 6,
        "sceMsgDialogOpen" => 6,
        "sceMsgDialogTerminate" => 6,
        "sceMsgDialogUpdateStatus" => 6,
    }
}
