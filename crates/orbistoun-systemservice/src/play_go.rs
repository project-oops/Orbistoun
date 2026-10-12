//! `libScePlayGo` - streaming a title's content in chunks while it installs.
//!
//! `scePlayGoInitialize` and `scePlayGoTerminate` are implemented, guest-observed (D815); nothing
//! else is.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libScePlayGo" {
        // (param): a pointer to a block the library works in, then its size.
        "scePlayGoInitialize" => 1,
        "scePlayGoTerminate" => 0,
    }
}

/// Whether the process has initialised PlayGo and not terminated it.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// `scePlayGoInitialize(param)`: 0, with nothing written (D815). The library binds in no probe leg
/// (REQ-pg01), so this is what the title's own code requires: PPSA21564 asserts the answer is 0 and
/// traps if not, and the retail title runs on the console. A null `param` is not that call and
/// answers the placeholder.
fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    INITIALIZED.store(true, Ordering::Release);
    0
}

/// `scePlayGoTerminate()`: 0 after [`initialize`] (D815). PPSA21564 terminates as it gives up on a
/// handle it could not open, and asserts the answer is 0 (`PlayGo.cpp` line 127). Before an
/// initialise it is unmeasured and answers the placeholder.
fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(false, Ordering::AcqRel) {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Implementations this module provides for `libScePlayGo`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("scePlayGoInitialize", initialize),
        ("scePlayGoTerminate", terminate),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Initialising with PPSA21564's block answers 0 and leaves the block as it was (D815).
    #[test]
    fn initialising_answers_zero_and_writes_nothing() {
        let work = vec![0xcc_u8; 0x100];
        let param = [work.as_ptr() as u64, 0x20_0000];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = param.as_ptr() as u64;
        assert_eq!(super::initialize(&args), 0);
        assert_eq!(param, [work.as_ptr() as u64, 0x20_0000]);
        assert!(work.iter().all(|&b| b == 0xcc));
        assert_eq!(
            super::terminate(&args),
            0,
            "terminating what was initialised"
        );
        assert_ne!(super::terminate(&args), 0, "and not twice");
    }
}
