//! `libSceMsgDialog.native` - the system message dialog.
//!
//! `sceMsgDialogInitialize` is implemented, as measured; the rest are declared only.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceMsgDialog.native" {
        "sceMsgDialogInitialize" => 6,
        "sceMsgDialogOpen" => 6,
        "sceMsgDialogTerminate" => 6,
        "sceMsgDialogUpdateStatus" => 6,
    }
}

/// Whether the process has initialised the message dialog.
static INITIALIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `sceMsgDialogInitialize()` - 0 the first time in a process, as a console answered after the
/// common dialog was set up (obSCEne `130-layout/common-dialog`,
/// `sceMsgDialogInitialize|return_code|0x0`, the klog leg's `obs.log` line 1956). A second call is
/// unmeasured and answers the placeholder.
pub(crate) fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        u64::from(GuestError::Unimplemented.as_raw())
    } else {
        0
    }
}

/// Implementations this module provides for `libSceMsgDialog.native`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceMsgDialogInitialize", initialize)]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The first `sceMsgDialogInitialize` answers 0, as a console did after the common dialog
    /// was set up (obSCEne `130-layout/common-dialog`, `sceMsgDialogInitialize|return_code|0x0`).
    #[test]
    fn the_message_dialog_initialises() {
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::initialize(&args), 0);
    }
}
