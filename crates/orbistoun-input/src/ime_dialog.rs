//! `libSceImeDialog` - the full-screen text-entry dialog over it.
//!
//! Served as obSCEne measured it on FW 12.40 (`REQ-20261012T1816Z-b874`, `000-boot/system-dialogs`,
//! rows in `reports/hardware/latest_probe_run.log` of 2026-10-13 15:06 - obSCEne `-3215`):
//! status 0 (none) before anything; `Init` refused `0x80bc0015` and the
//! status left at 0; result, abort and terminate refused `0x80bc0107` (not in use) with the result
//! block untouched. The refused `Init` was measured in a probe that had likely not initialised the
//! common dialog; once this process has, as every title does first, `Init` is unmeasured and
//! answers the placeholder. A dialog that never opens leaves every other answer as measured.
//!
//! Each arity is `6`, the trampoline's full capture (D504).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceImeDialog" {
        "sceImeDialogAbort" => 6,
        "sceImeDialogGetResult" => 6,
        "sceImeDialogGetStatus" => 6,
        "sceImeDialogInit" => 6,
        "sceImeDialogTerm" => 6,
    }
}

/// The dialog is not in use: nothing to abort, read or terminate (measured).
const NOT_IN_USE: u64 = 0x80bc_0107;
/// What `Init` answered in the measured process (measured).
const INIT_REFUSED: u64 = 0x80bc_0015;
/// The dialog's status with nothing open (measured).
const STATUS_NONE: u64 = 0;

/// `sceImeDialogGetStatus()`: 0, none, since no dialog ever opens here.
pub(crate) fn get_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    STATUS_NONE
}

/// `sceImeDialogGetResult(result)`: `0x80bc0107`, writing nothing.
pub(crate) fn get_result(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NOT_IN_USE
}

/// `sceImeDialogInit(param, extended)`: `0x80bc0015` where the common dialog is not initialised,
/// as measured; unmeasured once it is.
pub(crate) fn init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if orbistoun_systemservice::common_dialog::initialized() {
        u64::from(GuestError::Unimplemented.as_raw())
    } else {
        INIT_REFUSED
    }
}

/// `sceImeDialogAbort()`: `0x80bc0107`.
pub(crate) fn abort(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NOT_IN_USE
}

/// `sceImeDialogTerm()`: `0x80bc0107`, every time.
pub(crate) fn term(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NOT_IN_USE
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The measured sequence: status none, result refused with the block untouched, abort and
    /// terminate refused, terminate again refused.
    #[test]
    fn a_dialog_that_never_opens_answers_as_measured() {
        let none = [0_u64; GUEST_ARG_REGISTERS];
        let block = [0xcc_u8; 0x40];
        let mut with = none;
        with[0] = block.as_ptr() as u64;
        assert_eq!(super::get_status(&none), 0);
        assert_eq!(super::get_result(&with), 0x80bc_0107);
        assert_eq!(block, [0xcc; 0x40], "untouched");
        assert_eq!(super::abort(&none), 0x80bc_0107);
        assert_eq!(super::term(&none), 0x80bc_0107);
        assert_eq!(super::term(&none), 0x80bc_0107);
    }
}
