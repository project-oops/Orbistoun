//! `libSceAppContent` - downloadable content and additional-data mounts.
//!
//! The implemented names are the initialisation an IL2CPP title does at startup; the rest are
//! declared only. Arity is `6`, the trampoline's full capture, not a claim about how many
//! arguments each takes.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceAppContent" {
        "sceAppContentAddcontMount" => 6,
        "sceAppContentAddcontUnmount" => 6,
        "sceAppContentAppParamGetInt" => 6,
        "sceAppContentDownloadDataGetAvailableSpaceKb" => 6,
        "sceAppContentInitialize" => 6,
        "sceAppContentTemporaryDataFormat" => 6,
        "sceAppContentTemporaryDataGetAvailableSpaceKb" => 6,
        "sceAppContentTemporaryDataMount2" => 6,
    }
}

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// `sceAppContentInitialize(init, boot)` - starts the app-content subsystem. Answers `0`.
///
/// Guest-observed: `libil2cpp` calls it at startup and traps on the placeholder return. The
/// library is not reachable from a measurement context, so the guest's behaviour is the evidence.
/// `boot` is left unwritten because nothing measured says what it holds.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceAppContentAppParamGetInt(param_id, out)` - an integer app parameter, into `*out`.
///
/// Writes `0` and answers success, as `sceSystemServiceParamGetInt` does: the identifiers are
/// unmeasured, and zero reads as a first index, an unset flag or an empty count. The
/// out-parameter is always written, because an unwritten one hands the guest stale stack.
fn app_param_get_int(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[1];
    // Zero is the documented placeholder. The checked helper refuses address zero.
    if !super::write_word(out, 0) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceAppContentTemporaryDataMount2(option, mountPoint)` - mounts the title's temporary-data area.
/// Answers `0`.
///
/// Guest-observed on the same terms as [`initialize`]: a mount call fails on an error code and
/// proceeds on success. `mountPoint` is left unwritten because the `SceAppContentMountPoint`
/// layout and the path it would carry are unmeasured.
fn temporary_data_mount2(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Implementations this module provides for `libSceAppContent`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAppContentInitialize", initialize),
        ("sceAppContentAppParamGetInt", app_param_get_int),
        ("sceAppContentTemporaryDataMount2", temporary_data_mount2),
    ]
}
