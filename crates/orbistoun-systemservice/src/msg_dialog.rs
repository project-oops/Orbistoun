//! `libSceMsgDialog.native` - the system message dialog.
//!
//! `sceMsgDialogInitialize` is implemented, as measured. Opening, polling, reading the result of
//! and terminating a dialog are the shell's answer (D791): a dialog opened is shown to nobody, and
//! is reported finished with its OK button at the second status update.
//!
//! Every unmeasured arity is `6`, the trampoline's full capture, not a claim about how many
//! arguments a function takes: a wrong arity only degrades a trace, while a wrong name is
//! unreachable.

use std::sync::atomic::{AtomicU32, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceMsgDialog.native" {
        // (result)
        "sceMsgDialogGetResult" => 1,
        "sceMsgDialogInitialize" => 6,
        // (param)
        "sceMsgDialogOpen" => 1,
        "sceMsgDialogTerminate" => 0,
        "sceMsgDialogUpdateStatus" => 0,
    }
}

/// The common dialog's states, as `sceMsgDialogUpdateStatus` answers them (D791, assumed): none,
/// initialised, running, finished.
mod status {
    pub(super) const NONE: u32 = 0;
    pub(super) const INITIALIZED: u32 = 1;
    pub(super) const RUNNING: u32 = 2;
    pub(super) const FINISHED: u32 = 3;
}

/// The button a finished dialog reports pressed: OK (D791, assumed).
const BUTTON_OK: u32 = 1;

/// Where the process's message dialog is in its cycle, one of [`status`].
static STATE: AtomicU32 = AtomicU32::new(status::NONE);

/// `sceMsgDialogInitialize()` - 0 the first time in a process, as a console answered after the
/// common dialog was set up (obSCEne `130-layout/common-dialog`,
/// `sceMsgDialogInitialize|return_code|0x0`, the klog leg's `obs.log` line 1956). A second call
/// before the dialog is terminated is unmeasured and answers the placeholder.
pub(crate) fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match STATE.compare_exchange(
        status::NONE,
        status::INITIALIZED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => 0,
        Err(_) => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// `sceMsgDialogOpen(param)`: shows nothing and answers 0, the dialog now running (D791). Opening
/// one before initialising, or while one runs, answers the placeholder.
fn open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let opened = [status::INITIALIZED, status::FINISHED]
        .into_iter()
        .any(|from| {
            STATE
                .compare_exchange(from, status::RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        });
    if opened {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceMsgDialogUpdateStatus()`: the dialog's state. A running dialog reports running once and
/// then finished, its OK button pressed, as a user who dismissed it at once would leave it
/// (D791).
fn update_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    /// A running dialog that has been reported running once.
    const SEEN_RUNNING: u32 = 0x100 | status::RUNNING;
    let state = STATE.load(Ordering::Acquire);
    match state {
        status::RUNNING => {
            let _ = STATE.compare_exchange(
                status::RUNNING,
                SEEN_RUNNING,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            u64::from(status::RUNNING)
        }
        SEEN_RUNNING => {
            STATE.store(status::FINISHED, Ordering::Release);
            u64::from(status::FINISHED)
        }
        other => u64::from(other),
    }
}

/// `sceMsgDialogGetResult(result)`: for a finished dialog, writes the OK button's id to the
/// result's `buttonId` and a zero result code, and answers 0 (D791). The result is `{ mode u32,
/// result i32, buttonId u32, reserved }`; only `result` and `buttonId` are written. Before the
/// dialog finishes it answers the placeholder.
fn get_result(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let result = args[0];
    if result == 0 || STATE.load(Ordering::Acquire) != status::FINISHED {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's result block, by the call's contract.
    let written = unsafe { guest::write_u32(result.wrapping_add(4), 0) }
        // SAFETY: the same block's `buttonId`.
        && unsafe { guest::write_u32(result.wrapping_add(8), BUTTON_OK) };
    if written {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceMsgDialogTerminate()`: ends the dialog and answers 0; the next one starts with
/// `sceMsgDialogInitialize` again (D791).
fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    STATE.store(status::NONE, Ordering::Release);
    0
}

/// Implementations this module provides for `libSceMsgDialog.native`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceMsgDialogInitialize", initialize),
        ("sceMsgDialogOpen", open),
        ("sceMsgDialogUpdateStatus", update_status),
        ("sceMsgDialogGetResult", get_result),
        ("sceMsgDialogTerminate", terminate),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// One dialog's cycle as the shell answers it (D791): initialised, opened, reported running
    /// once and then finished, its result the OK button, and terminated back to none; the first
    /// `sceMsgDialogInitialize` answers 0, as a console did (`130-layout/common-dialog`).
    #[test]
    fn a_dialog_is_answered_at_once_with_ok() {
        let none = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::initialize(&none), 0);
        assert_eq!(super::update_status(&none), 1, "initialised");
        let param = [0x30_u8; 0x40];
        let mut args = none;
        args[0] = param.as_ptr() as u64;
        assert_eq!(super::open(&args), 0);
        assert_eq!(super::update_status(&none), 2, "running");
        let mut result = [0xa5_u8; 0x30];
        args[0] = result.as_mut_ptr() as u64;
        assert_ne!(super::get_result(&args), 0, "not finished yet");
        assert_eq!(super::update_status(&none), 3, "finished");
        assert_eq!(super::get_result(&args), 0);
        assert_eq!(result[4..8], [0; 4], "result code");
        assert_eq!(result[8..12], 1_u32.to_le_bytes(), "the OK button");
        assert_eq!(result[..4], [0xa5; 4], "mode left as it was");
        assert_eq!(super::terminate(&none), 0);
        assert_eq!(super::update_status(&none), 0, "none");
        assert_eq!(
            super::initialize(&none),
            0,
            "a terminated dialog initialises again"
        );
    }
}
