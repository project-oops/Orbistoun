//! `libSceWebBrowserDialog` - the embedded browser dialog.
//!
//! `sceWebBrowserDialogInitialize` answers 0, as an offline console's does - the dialog starts, and
//! only opening it would reach the network (see [`crate::offline`]). The rest are declared only.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_core::GuestFn;
use orbistoun_hle::guest_module;

guest_module! {
    "libSceWebBrowserDialog" {
        "sceWebBrowserDialogInitialize" => 6,
        "sceWebBrowserDialogOpen" => 6,
        "sceWebBrowserDialogTerminate" => 6,
        "sceWebBrowserDialogUpdateStatus" => 6,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceWebBrowserDialogInitialize", crate::offline::started)]
}
