//! `libSceErrorDialog` - the system error dialog.
//!
//! `sceErrorDialogInitialize` is served by the handler in `lib.rs`, which answers `OK`.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceErrorDialog" {
        "sceErrorDialogInitialize" => 6,
    }
}
