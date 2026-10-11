//! `libSceShare` - the system's share service (screenshots, clips, broadcast).
//!
//! `sceShareInitialize` and `sceShareTerminate` are implemented, as measured.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceShare" {
        // (size, priority, cpu mask)
        "sceShareInitialize" => 3,
        "sceShareTerminate" => 0,
    }
}

/// What `sceShareInitialize` answered once the process had initialised the service.
const ALREADY_INITIALIZED: u64 = 0x8196_000d;

/// Whether the process has initialised the share service.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

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
        assert_eq!(super::terminate(&[0; GUEST_ARG_REGISTERS]), 0);
    }
}
