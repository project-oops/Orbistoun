//! `libSceCommonDialog` - the shared initialisation every system dialog goes through.
//!
//! Arity is `6`, the trampoline's full capture, not a claim about how many arguments the function
//! takes; the real function takes none.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceCommonDialog" {
        "sceCommonDialogInitialize" => 6,
    }
}

/// Successful initialisation, as the guest reads it.
const OK: u64 = 0;

/// What a second initialisation in the same process answers: already initialised.
const ALREADY_INITIALIZED: u64 = 0x80b8_0002;

/// Whether the process has initialised the common dialog.
static INITIALIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `sceCommonDialogInitialize()` - starts the common-dialog subsystem: `0` the first time in a
/// process and `0x80b80002` after, as a console answered two calls in one process (obSCEne
/// `130-layout/common-dialog`, `20260923-dcb-extent-eboot.obs.log` 6399-6400). There is no
/// out-parameter.
pub(crate) fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        ALREADY_INITIALIZED
    } else {
        OK
    }
}

/// Whether this process has initialised the common dialog.
///
/// The system dialogs obSCEne measured (ErrorDialog, ImeDialog) were measured in a process that
/// may not have: their opens were refused, and whether that refusal stands once a title has
/// initialised the common dialog, as every title does first, is unmeasured.
#[must_use]
pub fn initialized() -> bool {
    INITIALIZED.load(std::sync::atomic::Ordering::Acquire)
}

/// Implementations this module provides for `libSceCommonDialog`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceCommonDialogInitialize", initialize)]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The first initialisation in a process answers 0 and every later one `0x80b80002`, the
    /// already-initialised code (obSCEne `130-layout/common-dialog`: `return_code 0x0`, then
    /// `return_code_2 0x80b80002`).
    #[test]
    fn the_common_dialog_initialises_once_per_process() {
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::initialize(&args), 0);
        assert_eq!(super::initialize(&args), 0x80b8_0002);
        assert_eq!(super::initialize(&args), 0x80b8_0002);
    }
}
