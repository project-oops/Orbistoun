//! `libSceSaveDataDialog.native` - the system's save-data dialog.
//!
//! The dialog is the shell's answer (D791), as the message dialog's is: one opened is shown to
//! nobody and reported finished with its OK button at the second status update, so a title that
//! asks the user about a save goes on as if they had answered at once. PPSA28061's save thread
//! opens one after mounting its save and polls `sceSaveDataDialogUpdateStatus` until it finishes;
//! unanswered, it polled twenty million times and the title went no further.
//!
//! Every unmeasured arity is `6`, the trampoline's full capture, not a claim about how many
//! arguments a function takes. obSCEne could not bind the library from homebrew (REQ-570e: the
//! module loaded, none of its symbols resolved), so nothing here is measured.

use std::sync::atomic::{AtomicU32, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceSaveDataDialog.native" {
        // (close param)
        "sceSaveDataDialogClose" => 1,
        // (result)
        "sceSaveDataDialogGetResult" => 1,
        "sceSaveDataDialogGetStatus" => 0,
        "sceSaveDataDialogInitialize" => 6,
        "sceSaveDataDialogIsReadyToDisplay" => 0,
        // (param)
        "sceSaveDataDialogOpen" => 1,
        "sceSaveDataDialogTerminate" => 0,
        "sceSaveDataDialogUpdateStatus" => 0,
    }
}

/// The common dialog's states, as the status calls answer them (D791, assumed): none,
/// initialised, running, finished.
mod status {
    pub(super) const NONE: u32 = 0;
    pub(super) const INITIALIZED: u32 = 1;
    pub(super) const RUNNING: u32 = 2;
    pub(super) const FINISHED: u32 = 3;
}

/// The button a finished dialog reports pressed: OK (D791, assumed).
const BUTTON_OK: u32 = 1;

/// A running dialog that has been reported running once.
const SEEN_RUNNING: u32 = 0x100 | status::RUNNING;

/// Where the process's save-data dialog is in its cycle, one of [`status`] or [`SEEN_RUNNING`].
static STATE: AtomicU32 = AtomicU32::new(status::NONE);

/// The placeholder, for every call made out of the dialog's order.
fn refused() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// `sceSaveDataDialogInitialize()`: 0, the dialog initialised; a second call before
/// `sceSaveDataDialogTerminate` answers the placeholder.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match STATE.compare_exchange(
        status::NONE,
        status::INITIALIZED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => 0,
        Err(_) => refused(),
    }
}

/// `sceSaveDataDialogOpen(param)`: shows nothing and answers 0, the dialog now running (D791).
/// A null parameter, or a dialog opened before initialising or while one runs, answers the
/// placeholder.
fn open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return refused();
    }
    let opened = [status::INITIALIZED, status::FINISHED]
        .into_iter()
        .any(|from| {
            STATE
                .compare_exchange(from, status::RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        });
    if opened { 0 } else { refused() }
}

/// The state a status call reports: a seen-running dialog still reads running.
fn reported(state: u32) -> u64 {
    u64::from(if state == SEEN_RUNNING {
        status::RUNNING
    } else {
        state
    })
}

/// `sceSaveDataDialogUpdateStatus()`: the dialog's state. A running dialog reports running once
/// and then finished, its OK button pressed, as a user who answered at once would leave it
/// (D791).
fn update_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match STATE.load(Ordering::Acquire) {
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

/// `sceSaveDataDialogGetStatus()`: the dialog's state, without moving it on.
fn get_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    reported(STATE.load(Ordering::Acquire))
}

/// `sceSaveDataDialogGetResult(result)`: for a finished dialog, writes a zero result code and the
/// OK button's id - `result` is `{ mode i32, result i32, buttonId i32, ... }`, and only those two
/// are written - and answers 0 (D791). Before the dialog finishes it answers the placeholder.
fn get_result(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let result = args[0];
    if result == 0 || STATE.load(Ordering::Acquire) != status::FINISHED {
        return refused();
    }
    // SAFETY: the guest's result block, by the call's contract; written through checked accessors.
    let written = unsafe { guest::write_u32(result.wrapping_add(4), 0) }
        // SAFETY: the same block's `buttonId`.
        && unsafe { guest::write_u32(result.wrapping_add(8), BUTTON_OK) };
    if written { 0 } else { refused() }
}

/// `sceSaveDataDialogClose(param)`: finishes a running dialog and answers 0; with none running
/// it answers the placeholder.
fn close(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let state = STATE.load(Ordering::Acquire);
    if state == status::RUNNING || state == SEEN_RUNNING {
        STATE.store(status::FINISHED, Ordering::Release);
        0
    } else {
        refused()
    }
}

/// `sceSaveDataDialogIsReadyToDisplay()`: 1, ready - nothing is ever waited for.
const fn is_ready_to_display(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    1
}

/// `sceSaveDataDialogTerminate()`: ends the dialog and answers 0; the next one starts with
/// `sceSaveDataDialogInitialize` again (D791).
fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    STATE.store(status::NONE, Ordering::Release);
    0
}

/// Implementations this module provides for `libSceSaveDataDialog.native`.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceSaveDataDialogInitialize", initialize),
        ("sceSaveDataDialogOpen", open),
        ("sceSaveDataDialogUpdateStatus", update_status),
        ("sceSaveDataDialogGetStatus", get_status),
        ("sceSaveDataDialogGetResult", get_result),
        ("sceSaveDataDialogClose", close),
        ("sceSaveDataDialogIsReadyToDisplay", is_ready_to_display),
        ("sceSaveDataDialogTerminate", terminate),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// One dialog's cycle as the shell answers it (D791): initialised, opened, reported running
    /// once and then finished, its result the OK button, closed and reopened, and terminated back
    /// to none. One test: the dialog's state is the process's.
    #[test]
    fn a_save_dialog_is_answered_at_once_with_ok() {
        let none = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::initialize(&none), 0);
        assert_ne!(super::initialize(&none), 0, "initialised already");
        assert_eq!(super::update_status(&none), 1, "initialised");
        let param = [0x30_u8; 0xa0];
        let mut args = none;
        args[0] = param.as_ptr() as u64;
        assert_eq!(super::open(&args), 0);
        assert_eq!(super::update_status(&none), 2, "running");
        assert_eq!(super::get_status(&none), 2, "still running");
        let mut result = [0xa5_u8; 0x40];
        let mut result_args = none;
        result_args[0] = result.as_mut_ptr() as u64;
        assert_ne!(super::get_result(&result_args), 0, "not finished yet");
        assert_eq!(super::update_status(&none), 3, "finished");
        assert_eq!(super::get_result(&result_args), 0);
        assert_eq!(result[4..8], [0; 4], "result code");
        assert_eq!(result[8..12], 1_u32.to_le_bytes(), "the OK button");
        assert_eq!(result[..4], [0xa5; 4], "mode left as it was");

        assert_eq!(super::open(&args), 0, "a second dialog");
        assert_eq!(super::close(&none), 0);
        assert_eq!(super::get_status(&none), 3, "closed is finished");
        assert_ne!(super::close(&none), 0, "nothing running");
        assert_eq!(super::is_ready_to_display(&none), 1);
        assert_eq!(super::terminate(&none), 0);
        assert_eq!(super::update_status(&none), 0, "none");
        assert_ne!(super::open(&args), 0, "not initialised");
    }
}
