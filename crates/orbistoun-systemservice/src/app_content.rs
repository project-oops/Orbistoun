//! `libSceAppContent` - downloadable content and additional-data mounts.
//!
//! **8 names from PPSA02664's import table.** Two are implemented - the initialisation a Unity
//! IL2CPP title does at startup - and the other six stay declared-only until a guest needs them.
//! Arity is `6`, the trampoline's full capture, not a claim about how many arguments each takes.

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
/// **Guest-observed, and the reason mirrors `sceCommonDialogInitialize` (D678).** PPSA25872
/// (Terminator 2D) is a Unity IL2CPP title; `libil2cpp` calls this early and, given the unimplemented
/// placeholder, prints `[libil2cpp] sceAppContentInitialize returned 0xf7ff0001` and traps (`int 0x41`).
/// Answering `0` clears that (the guest proceeds, verdict FURTHER). obSCEne's `130-layout/app-content`
/// resolved it to `0x0` on the payload and PKG legs, though `libSceAppContent` symbols are "not
/// available in this context" - the same context-gated measurement common-dialog hit - so guest-observed
/// is the ceiling of the evidence and answering `0` is the honest resolution.
///
/// The `boot` out-parameter is left unwritten: the oracle shows the guest proceeds without it (it did
/// not fault reading it), and nothing measured says what it holds, so a fabricated fill would be the
/// plausible-output principle 3 refuses.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceAppContentAppParamGetInt(param_id, out)` - an integer app parameter, into `*out`.
///
/// **Answers the documented placeholder, the same as `sceSystemServiceParamGetInt`.** The app-param
/// identifiers are console/title metadata nothing here has measured, so `0` is written - the value
/// least likely to send a guest somewhere surprising (an index reads as the first entry, a flag as off,
/// a count as none, all states a title already handles). Reporting success while writing the value is
/// what leaves a guest that checks the return, and one that does not, both in a state they can handle.
/// `libil2cpp` calls this right after initialize and fails on the placeholder return, so answering it is
/// the next step of the same startup sequence.
///
/// The out-parameter must be written either way: unwritten, the guest reads whatever its stack held,
/// which is a different wrong answer every run with no signature (the failure `param_get_int` documents).
fn app_param_get_int(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[1];
    // Zero is the documented placeholder; see the doc comment. Written through the parent's checked
    // helper, which refuses address zero rather than faulting inside the emulator.
    if !super::write_word(out, 0) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceAppContentTemporaryDataMount2(option, mountPoint)` - mounts the title's temporary-data area and
/// fills `*mountPoint` with where it landed. Answers `0`.
///
/// **Guest-observed, the same context-gated ceiling as [`initialize`].** PPSA25872 (Terminator 2D, a
/// Unity IL2CPP title) calls it, and unimplemented it answered the `0xf7ff0001` placeholder. obSCEne
/// cannot measure `libSceAppContent` directly - its symbols are "not available in this context", the gate
/// [`initialize`] and common-dialog also hit - so guest-observed is the ceiling and answering `0` is the
/// honest resolution: a mount call fails on an error code and proceeds on success, and a placeholder is a
/// lie either way. Implemented on the terms [`app_param_get_int`] states: answer honestly whether or not
/// it is the call blocking the next wall.
///
/// **It is not, for this title, and that is recorded rather than glossed** (worklog 673): replacing the
/// placeholder here did not move PPSA25872's wall. It still dies at the same `int 0x41` assert
/// (image+0x17554a3) reached through a *different* chain of unimplemented stubs (`sceUserServiceGetAgeLevel`
/// and others) and a string it formats about a failed lookup - a cause deeper than any one placeholder.
/// This closes one honest gap; it does not clear that wall.
///
/// The `mountPoint` out-parameter is left unwritten, on the same terms as [`initialize`]'s `boot`:
/// nothing measured says the `SceAppContentMountPoint` layout or the path a temporary-data mount answers,
/// and a fabricated path would be the plausible output principle 3 refuses.
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
