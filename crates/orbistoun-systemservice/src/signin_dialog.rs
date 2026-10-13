//! `libSceSigninDialog` - the system sign-in dialog.
//!
//! Initialise and terminate are as obSCEne measured them on FW 12.40
//! (`REQ-20260926T1952Z-737e`, `070-user/signin-dialog-lifecycle`, `report-1791727748.txt`): each
//! answers 0, and the library initialises again after terminating. The status values are the
//! common dialog's (none 0, initialised 1, running 2, finished 3), and a dialog opened is the
//! shell's answer (D791): shown to nobody, reported running once and then finished, as a user who
//! dismissed it at once and carried on offline would leave it. Those are assumed until the status
//! arm is measured. The result of a finished dialog is declared and unimplemented.
//!
//! Every unmeasured arity is `6`, the trampoline's full capture, not a claim about how many
//! arguments a function takes: a wrong arity only degrades a trace, while a wrong name is
//! unreachable.

use std::sync::atomic::{AtomicU32, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceSigninDialog" {
        "sceSigninDialogInitialize" => 6,
        // (param)
        "sceSigninDialogOpen" => 1,
        "sceSigninDialogUpdateStatus" => 0,
        "sceSigninDialogGetStatus" => 0,
        "sceSigninDialogGetResult" => 6,
        "sceSigninDialogClose" => 0,
        "sceSigninDialogTerminate" => 0,
    }
}

/// The common dialog's states, as the status calls answer them (assumed): none, initialised,
/// running, finished.
mod status {
    pub(super) const NONE: u32 = 0;
    pub(super) const INITIALIZED: u32 = 1;
    pub(super) const RUNNING: u32 = 2;
    pub(super) const FINISHED: u32 = 3;
}

/// A running dialog that has been reported running once.
const SEEN_RUNNING: u32 = 0x100 | status::RUNNING;

/// Where the process's sign-in dialog is in its cycle, one of [`status`] or [`SEEN_RUNNING`].
static STATE: AtomicU32 = AtomicU32::new(status::NONE);

fn unmeasured() -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// The state as the status calls report it.
fn reported(state: u32) -> u64 {
    u64::from(if state == SEEN_RUNNING {
        status::RUNNING
    } else {
        state
    })
}

/// `sceSigninDialogInitialize()`: 0, measured the first time and again after a terminate. A
/// second call while initialised is unmeasured and answers the placeholder.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match STATE.compare_exchange(
        status::NONE,
        status::INITIALIZED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => 0,
        Err(_) => unmeasured(),
    }
}

/// `sceSigninDialogOpen(param)`: shows nothing and answers 0, the dialog now running (D791).
/// Opening one before initialising, while one runs, or with no parameter answers the placeholder.
fn open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return unmeasured();
    }
    let opened = [status::INITIALIZED, status::FINISHED]
        .into_iter()
        .any(|from| {
            STATE
                .compare_exchange(from, status::RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        });
    if opened { 0 } else { unmeasured() }
}

/// `sceSigninDialogUpdateStatus()`: the dialog's state, returned rather than written. A running
/// dialog reports running once and then finished (D791).
fn update_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let state = STATE.load(Ordering::Acquire);
    match state {
        status::RUNNING => {
            let _ = STATE.compare_exchange(
                status::RUNNING,
                SEEN_RUNNING,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        SEEN_RUNNING => {
            STATE.store(status::FINISHED, Ordering::Release);
            return u64::from(status::FINISHED);
        }
        _ => {}
    }
    reported(state)
}

/// `sceSigninDialogGetStatus()`: the dialog's state, without advancing it.
fn get_status(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    reported(STATE.load(Ordering::Acquire))
}

/// `sceSigninDialogClose()`: ends a running dialog, which then reports finished, and answers 0.
/// With nothing open it answers the placeholder.
fn close(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let closed = [status::RUNNING, SEEN_RUNNING, status::FINISHED]
        .into_iter()
        .any(|from| {
            STATE
                .compare_exchange(from, status::FINISHED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        });
    if closed { 0 } else { unmeasured() }
}

/// `sceSigninDialogTerminate()`: 0 once initialised, measured, after which the library
/// initialises again. Before initialising it answers the placeholder.
fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if STATE.swap(status::NONE, Ordering::AcqRel) == status::NONE {
        unmeasured()
    } else {
        0
    }
}

/// Implementations this module provides for `libSceSigninDialog`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceSigninDialogInitialize", initialize),
        ("sceSigninDialogOpen", open),
        ("sceSigninDialogUpdateStatus", update_status),
        ("sceSigninDialogGetStatus", get_status),
        ("sceSigninDialogClose", close),
        ("sceSigninDialogTerminate", terminate),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The measured initialise, terminate and second initialise, with the shell's dialog between:
    /// opened, reported running once and then finished, and the status read without advancing.
    #[test]
    fn the_signin_dialog_lifecycle() {
        let none = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::get_status(&none), 0, "none before initialising");
        assert_eq!(super::initialize(&none), 0);
        assert_eq!(super::update_status(&none), 1, "initialised");
        let param = [0x10_u8, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut args = none;
        args[0] = param.as_ptr() as u64;
        assert_eq!(super::open(&args), 0);
        assert_eq!(super::get_status(&none), 2, "running");
        assert_eq!(super::update_status(&none), 2, "running, reported once");
        assert_eq!(super::get_status(&none), 2, "reading does not advance");
        assert_eq!(super::update_status(&none), 3, "finished");
        assert_eq!(super::close(&none), 0);
        assert_eq!(super::get_status(&none), 3);
        assert_eq!(super::terminate(&none), 0);
        assert_eq!(super::get_status(&none), 0, "none");
        assert_eq!(
            super::initialize(&none),
            0,
            "initialises again, as measured"
        );
        assert_ne!(super::initialize(&none), 0, "not twice");
        assert_eq!(super::terminate(&none), 0);
    }
}
