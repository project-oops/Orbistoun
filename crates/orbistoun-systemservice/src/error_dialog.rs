//! `libSceErrorDialog` - the system error dialog.
//!
//! The lifecycle is as obSCEne measured it on FW 12.40 (`REQ-20261012T1822Z-bc38`,
//! `000-boot/system-dialogs`, `report-1791878041.txt`): the library initialises once; an open with
//! a well-formed parameter is refused `0x80ed0007`; status reads 1, initialised and not running,
//! before and after; and close and terminate succeed. The refused open was measured in a probe
//! that had likely not initialised the common dialog (obSCEne's own reading is that a process
//! with no display layer is shown nothing), so once this process has initialised it - as every
//! title does - the open is unmeasured. What was not measured - that open, an open before
//! initialising, a null parameter, a status after terminating - answers the placeholder rather
//! than a guess.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceErrorDialog" {
        "sceErrorDialogInitialize" => 6,
        "sceErrorDialogOpen" => 6,
        "sceErrorDialogGetStatus" => 6,
        "sceErrorDialogUpdateStatus" => 6,
        "sceErrorDialogClose" => 6,
        "sceErrorDialogTerminate" => 6,
    }
}

/// A second `Initialize` (measured).
const ALREADY_INITIALIZED: u64 = 0x80ed_0002;
/// An open whose parameter does not say it is 16 bytes (measured).
const INVALID_PARAM_SIZE: u64 = 0x80ed_0003;
/// A well-formed open in a process that shows no dialog (measured).
const NOT_SHOWN: u64 = 0x80ed_0007;

/// The size an open's parameter must state in its first word (measured).
const PARAM_SIZE: u32 = 16;

/// The status the library reports once initialised: initialised, not running (measured).
const STATUS_INITIALIZED: u64 = 1;

/// Whether `Initialize` has been called in this process and `Terminate` has not.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

fn unmeasured() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// `sceErrorDialogInitialize()`: 0 the first time, `0x80ed0002` while initialised.
pub(crate) fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        ALREADY_INITIALIZED
    } else {
        0
    }
}

/// `sceErrorDialogOpen(param)`: `0x80ed0003` for a parameter not stating 16 bytes, and
/// `0x80ed0007` for one that does in a process that has not initialised the common dialog; once it
/// has, unmeasured.
pub(crate) fn open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return unmeasured();
    }
    // SAFETY: the parameter the guest passed for this call, valid by its contract.
    match unsafe { guest::read_u32(args[0]) } {
        Some(PARAM_SIZE) if crate::common_dialog::initialized() => unmeasured(),
        Some(PARAM_SIZE) => NOT_SHOWN,
        Some(_) => INVALID_PARAM_SIZE,
        None => unmeasured(),
    }
}

/// `sceErrorDialogGetStatus()`: 1, initialised and not running, before and after an open.
pub(crate) fn get_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.load(Ordering::Relaxed) {
        STATUS_INITIALIZED
    } else {
        unmeasured()
    }
}

/// `sceErrorDialogUpdateStatus()`: the same status, measured the same.
pub(crate) fn update_status(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    get_status(args)
}

/// `sceErrorDialogClose()`: 0, and the status stays initialised.
pub(crate) fn close(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.load(Ordering::Relaxed) {
        0
    } else {
        unmeasured()
    }
}

/// `sceErrorDialogTerminate()`: 0, after which the library may be initialised again.
pub(crate) fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(false, Ordering::Relaxed) {
        0
    } else {
        unmeasured()
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The measured lifecycle, in the order obSCEne ran it: initialise, initialise again, an open
    /// with a bad size and one with a good size, status, update, close, status, terminate.
    #[test]
    fn the_error_dialog_lifecycle_is_as_measured() {
        let none = [0_u64; GUEST_ARG_REGISTERS];
        let with = |param: &[u32; 4]| {
            let mut args = none;
            args[0] = param.as_ptr() as u64;
            args
        };
        assert_eq!(super::initialize(&none), 0);
        assert_eq!(super::initialize(&none), 0x80ed_0002);
        assert_eq!(super::open(&with(&[8, 0x8002_0001, 0, 0])), 0x80ed_0003);
        // Measured only where the common dialog was not initialised; another test in this process
        // may have initialised it, after which the open is unmeasured.
        let expected = if crate::common_dialog::initialized() {
            super::unmeasured()
        } else {
            0x80ed_0007
        };
        assert_eq!(super::open(&with(&[16, 0x8002_0001, 0, 0])), expected);
        assert_eq!(super::get_status(&none), 1);
        assert_eq!(super::update_status(&none), 1);
        assert_eq!(super::close(&none), 0);
        assert_eq!(super::get_status(&none), 1, "still initialised after close");
        assert_eq!(super::terminate(&none), 0);
        assert_eq!(super::initialize(&none), 0, "initialisable again");
        assert_eq!(super::terminate(&none), 0);
    }
}
