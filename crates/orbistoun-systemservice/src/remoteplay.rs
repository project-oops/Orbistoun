//! `libSceRemoteplay` - remote play.
//!
//! `sceRemoteplayInitialize`, `sceRemoteplayGetConnectionStatus` and `sceRemoteplayTerminate` are
//! implemented, as measured (REQ-so01, `report-1791801495.txt` 156-167, link-bound in a fresh
//! process).

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceRemoteplay" {
        // (user, status)
        "sceRemoteplayGetConnectionStatus" => 2,
        // (heap, size)
        "sceRemoteplayInitialize" => 2,
        "sceRemoteplayTerminate" => 0,
    }
}

/// What `sceRemoteplayInitialize` answered once the process had initialised remote play.
const ALREADY_INITIALIZED: u64 = 0x80fc_0003;

/// Whether the process has initialised remote play.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// `sceRemoteplayInitialize(heap, size)`: 0 the first time in a process and `0x80fc0003` after,
/// with PPSA04263's arguments - a 0x1800-byte heap, 16-byte aligned. The console wrote 0x55c bytes
/// of its own state into the heap, which is the library's to read and not the title's; nothing is
/// written here.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        ALREADY_INITIALIZED
    } else {
        0
    }
}

/// `sceRemoteplayGetConnectionStatus(user, status)`: 0, with the four bytes of `status` written as
/// 0 - no remote-play session - after initialisation, for the initial user. Before it, or with no
/// status, it is unmeasured and answers the placeholder.
fn get_connection_status(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let status = args[1];
    if status == 0 || !INITIALIZED.load(Ordering::Acquire) {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's status word, by the call's contract.
    if unsafe { guest::write_u32(status, 0) } {
        0
    } else {
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    }
}

/// `sceRemoteplayTerminate()`: 0, as it answered after the calls above.
fn terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    INITIALIZED.store(false, Ordering::Release);
    0
}

/// Implementations this module provides for `libSceRemoteplay`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceRemoteplayInitialize", initialize),
        ("sceRemoteplayGetConnectionStatus", get_connection_status),
        ("sceRemoteplayTerminate", terminate),
    ]
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Remote play initialises once per process, as REQ-so01 measured: 0, then `0x80fc0003`; the
    /// connection status is 0 in four bytes, the rest of the word as it was; terminating answers 0.
    #[test]
    fn remote_play_initialises_once_and_reports_no_connection() {
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        let heap = vec![0xcc_u8; 0x1800];
        args[..2].copy_from_slice(&[heap.as_ptr() as u64, 0x1800]);
        assert_eq!(super::initialize(&args), 0);
        assert_eq!(super::initialize(&args), 0x80fc_0003);
        let mut status = [0xcc_u8; 8];
        args[..2].copy_from_slice(&[0x1ea2_f4d9, status.as_mut_ptr() as u64]);
        assert_eq!(super::get_connection_status(&args), 0);
        assert_eq!(status, [0, 0, 0, 0, 0xcc, 0xcc, 0xcc, 0xcc]);
        assert_eq!(super::terminate(&args), 0);
    }
}
