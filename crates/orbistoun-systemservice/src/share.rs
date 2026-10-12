//! `libSceShare` - the system's share service (screenshots, clips, broadcast).
//!
//! `sceShareInitialize`, `sceShareTerminate` and `sceShareRegisterContentEventCallback` are
//! implemented, as measured.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceShare" {
        // (size, priority, cpu mask)
        "sceShareInitialize" => 3,
        // (callbacks)
        "sceShareRegisterContentEventCallback" => 1,
        "sceShareTerminate" => 0,
    }
}

/// What `sceShareInitialize` answered once the process had initialised the service.
const ALREADY_INITIALIZED: u64 = 0x8196_000d;

/// Whether the process has initialised the share service.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// What `sceShareRegisterContentEventCallback` answered with its callbacks already registered.
const ALREADY_REGISTERED: u64 = 0x8196_0002;

/// Whether content-event callbacks are registered.
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// `sceShareRegisterContentEventCallback(callbacks)`: 0 the first time after the service is
/// initialised, leaving the 0x30-byte block untouched, and `0x81960002` after (REQ-sh02,
/// `report-1791758178.txt` 3856-3879, link-bound with PPSA21564's block: two function pointers then
/// zeros). Neither callback was called in the two seconds watched, and nothing here calls them.
/// Before initialisation, or with no block, it is unmeasured and answers the placeholder.
fn register_content_event_callback(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 || !INITIALIZED.load(Ordering::Acquire) {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    }
    if REGISTERED.swap(true, Ordering::AcqRel) {
        ALREADY_REGISTERED
    } else {
        0
    }
}

/// `sceShareInitialize(size, priority, cpu_mask)`: 0 the first time in a process and `0x8196000d`
/// after, link-bound in a fresh process with PPSA21564's arguments `(0x20000, 0x2bc, 0x1fff)`
/// (REQ-sh01, `report-1791727748.txt` 3717-3729: the first call answered 0 before sysmodule
/// 0x10f was loaded, and both calls after it `0x8196000d`).
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        ALREADY_INITIALIZED
    } else {
        0
    }
}

/// `sceShareTerminate()`: 0, as it answered after the calls above (REQ-sh01).
fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    INITIALIZED.store(false, Ordering::Release);
    0
}

/// Implementations this module provides for `libSceShare`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceShareInitialize", initialize),
        ("sceShareTerminate", terminate),
        (
            "sceShareRegisterContentEventCallback",
            register_content_event_callback,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The share service initialises once per process, as REQ-sh01 measured: 0, then
    /// `0x8196000d`; terminating answers 0.
    #[test]
    fn the_share_service_initialises_once_as_measured() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[..3].copy_from_slice(&[0x20000, 0x2bc, 0x1fff]);
        assert_eq!(super::initialize(&args), 0);
        assert_eq!(super::initialize(&args), 0x8196_000d);
        // Content-event callbacks register once (REQ-sh02): 0, then `0x81960002`, the block
        // untouched.
        let block = [0x4a_e020_u64, 0x4a_e080, 0, 0, 0, 0];
        let mut callbacks = [0_u64; GUEST_ARG_REGISTERS];
        callbacks[0] = block.as_ptr() as u64;
        assert_eq!(super::register_content_event_callback(&callbacks), 0);
        assert_eq!(
            super::register_content_event_callback(&callbacks),
            0x8196_0002
        );
        assert_eq!(block, [0x4a_e020, 0x4a_e080, 0, 0, 0, 0]);
        assert_eq!(super::terminate(&[0; GUEST_ARG_REGISTERS]), 0);
    }
}
