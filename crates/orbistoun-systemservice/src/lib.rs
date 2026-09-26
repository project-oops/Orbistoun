//! System service HLE - the settings and status a title asks the system about.
//!
//! A title asks which language is set, which button confirms and what the display looks like.
//! The answers are settings of the machine, not facts about the guest, and most are unmeasured.
//! The interface answers through out-pointers, so an unwritten one leaves the guest reading stale
//! stack, a different wrong answer every run. Every out-pointer is therefore written, with a
//! stated placeholder where the value is unknown (D171).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

pub mod app_content;
pub mod common_dialog;
pub mod coredump;
pub mod error_dialog;
pub mod json2;
pub mod launch;
pub mod msg_dialog;
pub mod remoteplay;
pub mod save_data;
pub mod web_browser_dialog;

pub mod console;

guest_module! {
    "libSceSystemService" {
        "sceSystemServiceParamGetInt" => 2,
        "sceSystemServiceHideSplashScreen" => 0,
        "sceSystemServiceGetStatus" => 1,
        // title id, argv, parameter block.
        "sceSystemServiceLaunchApp" => 3,
        // A system-flag setter. The real signature is unmeasured, so the arity is the trampoline's
        // six; the handler reads none of it.
        "sceSystemServiceDisableNoticeScreenSkipFlagAutoSet" => 6,
    }
}

/// The user service, which is its own library.
///
/// A nested module because `guest_module!` names its declaration `MODULE`, and a crate serving
/// two libraries needs two of them.
pub mod user {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceUserService" {
            "sceUserServiceInitialize" => 1,
            "sceUserServiceTerminate" => 0,
            "sceUserServiceGetInitialUser" => 1,
            "sceUserServiceGetUserName" => 3,
            // Declared so a trace names them, and not implemented: each writes a number or
            // structure whose meaning is unmeasured (an age band, an accessibility encoding, a list
            // layout) (D346).
            "sceUserServiceGetLoginUserIdList" => 1,
            "sceUserServiceGetAgeLevel" => 2,
            "sceUserServiceGetGamePresets" => 2,
            "sceUserServiceGetEvent" => 1,
            "sceUserServiceGetAccessibilityVibration" => 2,
            "sceUserServiceGetAccessibilityTriggerEffect" => 2,
            "sceUserServiceGetAccessibilityPressAndHoldDelay" => 2,
            "sceUserServiceGetAccessibilityChatTranscription" => 2,
        }
    }
}

/// Loading system modules - `libSceSysmodule`.
///
/// A nested module for the same reason as [`user`]. Nearly every title calls it first to bring a
/// library in before using it.
pub mod sysmodule {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceSysmodule" {
            "sceSysmoduleLoadModule" => 1,
            "sceSysmoduleUnloadModule" => 1,
            "sceSysmoduleIsLoaded" => 1,
        }
    }
}

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// What an unknown system parameter answers.
///
/// A placeholder, not a value: the identifiers are machine settings no lawful source describes.
/// Zero reads as a first index, an unset flag or an empty count, all states a title handles.
const UNKNOWN_PARAMETER: u64 = 0;

/// Writes a machine word into guest memory. Guest memory is identity-mapped.
fn write_word(address: u64, value: u64) -> bool {
    let Ok(at) = usize::try_from(address) else {
        return false;
    };
    if at == 0 {
        return false;
    }
    // SAFETY: the guest supplied this destination, the same contract the real call has. Written
    // unaligned because alignment is the guest's; an unmapped address faults as it would in the
    // guest.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at),
            value as u32,
        );
    }
    true
}

/// `sceSystemServiceParamGetInt(param, out)`.
///
/// The out-pointer is always written, because an unwritten one hands the guest stale stack that
/// differs every run and leaves no placeholder in a trace. It answers `OK`: a guest that checks the
/// return proceeds, and one that does not reads the written value either way.
fn param_get_int(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[1];
    // A measured answer if the machine has one, the placeholder otherwise; counted either way
    // (`console::summarise`). An identifier too large to be one takes the unmeasured path.
    let value = u32::try_from(args[0])
        .ok()
        .and_then(console::parameter)
        .map_or(UNKNOWN_PARAMETER, |answer| {
            // Through the bytes rather than `as`: a negative measured `int` reaches the guest as
            // its bits.
            u64::from(u32::from_ne_bytes(answer.to_ne_bytes()))
        });
    if !write_word(out, value) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// The user the machine is signed in as.
///
/// An assumed identifier: titles key save data on it, so the number matters. One is the first
/// identifier a one-user machine hands out, and zero reads as nobody.
const INITIAL_USER: u64 = 1;

/// `sceUserServiceInitialize(params)` - starts the user service. Accepts any parameters and reads
/// none of them.
fn user_service_initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceUserServiceTerminate()`.
fn user_service_terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceErrorDialogInitialize()` - sets up the system error-dialog subsystem.
///
/// Answers `OK`, as an SDK `*Initialize` does, without claiming a dialog was shown. Arity is the
/// trampoline's six, not a claim about the real signature.
fn error_dialog_initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceSystemServiceHideSplashScreen()` - dismisses the boot splash once a title is ready to draw.
///
/// Nothing displays a splash layer here, so hiding it is a no-op that succeeds, like
/// [`sysmodule_unload_module`]. Answers `OK` and writes nothing.
fn hide_splash_screen(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceSystemServiceDisableNoticeScreenSkipFlagAutoSet()` - turns off a system flag's auto-set.
///
/// A setter's contract is that the request was taken. orbistoun keeps no such flag, so nothing
/// is toggled or written and the call answers `OK`. The handler reads none of its arguments.
fn disable_notice_screen_skip_flag_auto_set(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Longest name this will write, however large a buffer says it is.
///
/// A ceiling on trust in the size argument: a larger value is treated as not a size at all, which
/// is what a wrong argument position looks like. A longer name is truncated, not refused.
const MAX_USER_NAME: usize = 64;

/// `sceUserServiceGetUserName(user, out, size)` - the name a person chose, for a guest.
///
/// The name is a string the owner types in the shell and the guest reads unencoded (D346).
/// `size` being the third argument is an assumption, and a wrong one writes past a caller's
/// buffer, so it is believed only within [`MAX_USER_NAME`]; outside that the call is refused.
fn user_service_get_user_name(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(id) = u32::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    let Some(name) = console::settings().user(id).map(|user| user.name.clone()) else {
        // A user this machine does not have is refused, not given the signed-in user's name.
        return u64::from(GuestError::InvalidArgument.as_raw());
    };

    let out = args[1];
    let Ok(size) = usize::try_from(args[2]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // Room for at least one byte of name and its terminator, and no more than this shim believes.
    if !(2..=MAX_USER_NAME).contains(&size) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(at) = usize::try_from(out) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if at == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }

    // Truncated on a character boundary so the guest receives valid text.
    let room = size - 1;
    let mut keep = room.min(name.len());
    while keep > 0 && !name.is_char_boundary(keep) {
        keep -= 1;
    }

    let destination = std::ptr::with_exposed_provenance_mut::<u8>(at);
    // SAFETY: the guest supplied this identity-mapped destination and its size, the real call's
    // contract. `keep` is at most `size - 1` and `size` is bounded, so the write stays inside the
    // buffer. The name is owned locally and cannot overlap it.
    unsafe {
        std::ptr::copy_nonoverlapping(name.as_ptr(), destination, keep);
    }
    // SAFETY: `keep < size` and the buffer is `size` bytes, so the terminator byte is inside it.
    let terminator = unsafe { destination.add(keep) };
    // SAFETY: the same byte, inside the caller's buffer.
    unsafe {
        terminator.write(0);
    }
    OK
}

/// The identifier of whoever is signed in, as every call that reports it answers.
///
/// One accessor, so `sceUserServiceGetInitialUser` and `sceUserServiceGetLoginUserIdList` cannot
/// disagree and have a title key save data and session on different users.
fn signed_in_user() -> u32 {
    // A machine with a deleted signed-in user answers the placeholder (D346).
    console::settings()
        .current()
        .map_or(INITIAL_USER as u32, |user| user.id)
}

/// `sceUserServiceGetLoginUserIdList(out)` - which users are signed in.
///
/// The length of the caller's structure is unmeasured. orbistoun signs in exactly one user, so
/// this writes one identifier and leaves the rest as the caller had it: zero-filling a tail would
/// invent a length. A caller that does not clear the structure reads its own stale bytes, a
/// visible wrong answer rather than a hidden one.
fn user_service_get_login_user_id_list(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(at) = usize::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: a guest-supplied `int *` in identity-mapped guest memory, written at its declared
    // four-byte width (D272).
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at),
            signed_in_user(),
        );
    }
    OK
}

/// `sceUserServiceGetInitialUser(out)` - the user a title should start as.
fn user_service_get_initial_user(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(at) = usize::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // SAFETY: a guest-supplied `int *` in identity-mapped guest memory, written at its declared
    // four-byte width (D272).
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<u32>(at),
            // The configured signed-in user; a deleted one answers the placeholder (D346).
            signed_in_user(),
        );
    }
    OK
}

/// `sceSysmoduleLoadModule(id)` - brings a system module in so its functions can be called.
///
/// Always succeeds: the loader resolves every library a title imports before the guest runs, so
/// the requested module is already callable. Answers `0`; a positive placeholder would read as a
/// module handle the guest might dereference.
fn sysmodule_load_module(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceSysmoduleUnloadModule(id)` - a no-op that succeeds, since loading paged nothing in.
fn sysmodule_unload_module(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `SCE_SYSMODULE_ERROR_UNLOADED` - answered for an identifier that names no loaded module.
///
/// Measured: `sceSysmoduleIsLoaded(0)` answers the sysmodule error base `0x805a_0000` with its
/// unloaded code (obSCEne's `060-module/sysmodule-query`).
const SYSMODULE_UNLOADED: u64 = 0x805a_1000;

/// `sceSysmoduleIsLoaded(id)` - whether a module is loaded.
///
/// A valid identifier answers `0` (loaded), since the loader resolves every module. Identifier 0
/// is not a loadable module and answers the measured [`SYSMODULE_UNLOADED`].
fn sysmodule_is_loaded(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return SYSMODULE_UNLOADED;
    }
    OK
}

/// Implementations this crate provides, by symbol name. Names rather than hashes, so the table can
/// be read and checked against the declarations.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceUserServiceInitialize", user_service_initialize),
        ("sceUserServiceTerminate", user_service_terminate),
        (
            "sceUserServiceGetInitialUser",
            user_service_get_initial_user,
        ),
        (
            "sceUserServiceGetLoginUserIdList",
            user_service_get_login_user_id_list,
        ),
        ("sceUserServiceGetUserName", user_service_get_user_name),
        ("sceErrorDialogInitialize", error_dialog_initialize),
        ("sceSystemServiceParamGetInt", param_get_int),
        ("sceSystemServiceHideSplashScreen", hide_splash_screen),
        (
            "sceSystemServiceDisableNoticeScreenSkipFlagAutoSet",
            disable_notice_screen_skip_flag_auto_set,
        ),
        ("sceSysmoduleLoadModule", sysmodule_load_module),
        ("sceSysmoduleUnloadModule", sysmodule_unload_module),
        ("sceSysmoduleIsLoaded", sysmodule_is_loaded),
    ]
}

#[cfg(test)]
mod tests {
    use super::{MAX_USER_NAME, UNKNOWN_PARAMETER, implementations, param_get_int};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Arguments for a name request against a real buffer.
    fn name_call(user: u64, buffer: &mut [u8], size: u64) -> [u64; GUEST_ARG_REGISTERS] {
        let mut args = [0; GUEST_ARG_REGISTERS];
        args[0] = user;
        args[1] = buffer.as_mut_ptr() as u64;
        args[2] = size;
        args
    }

    /// The two calls that report who is signed in never disagree.
    #[test]
    fn the_login_list_names_the_same_user_as_the_initial_user() {
        let mut list = [0xCCu32; 4];
        let mut one = [0xCCu32; 4];

        let mut args = [0; GUEST_ARG_REGISTERS];
        args[0] = list.as_mut_ptr() as u64;
        assert_eq!(super::user_service_get_login_user_id_list(&args), 0);

        let mut args = [0; GUEST_ARG_REGISTERS];
        args[0] = one.as_mut_ptr() as u64;
        assert_eq!(super::user_service_get_initial_user(&args), 0);

        assert_eq!(
            list[0], one[0],
            "the list's first entry is the signed-in user"
        );
        assert_ne!(list[0], 0xCC, "and something was actually written");
    }

    /// The login list writes exactly one four-byte identifier (D272) and leaves the tail alone.
    #[test]
    fn only_the_first_identifier_is_written() {
        let mut list = [0xCCu32; 4];
        let mut args = [0; GUEST_ARG_REGISTERS];
        args[0] = list.as_mut_ptr() as u64;

        assert_eq!(super::user_service_get_login_user_id_list(&args), 0);
        assert_eq!(
            &list[1..],
            &[0xCCu32; 3],
            "the tail is the caller's, because its length is not established"
        );
    }

    /// A null structure is refused rather than written through.
    #[test]
    fn a_null_login_list_is_refused() {
        let args = [0; GUEST_ARG_REGISTERS];
        assert_ne!(
            super::user_service_get_login_user_id_list(&args),
            0,
            "writing through null is not an option"
        );
    }

    /// A size outside what a name could need is refused, not acted on (D346).
    #[test]
    fn a_size_this_shim_does_not_believe_is_refused() {
        let mut buffer = [0xAA_u8; 8];

        for claimed in [0, 1, MAX_USER_NAME as u64 + 1, u64::MAX] {
            let refused = super::user_service_get_user_name(&name_call(1, &mut buffer, claimed));
            assert_ne!(
                refused,
                super::OK,
                "a claimed size of {claimed} was acted on"
            );
        }
        assert!(
            buffer.iter().all(|byte| *byte == 0xAA),
            "and nothing was written to the buffer while refusing"
        );
    }

    /// A user this machine does not have is refused rather than answered with somebody else.
    #[test]
    fn an_unknown_user_is_not_answered_with_the_signed_in_ones_name() {
        let mut buffer = [0_u8; 32];
        let refused = super::user_service_get_user_name(&name_call(9999, &mut buffer, 32));

        assert_ne!(refused, super::OK);
        assert_eq!(buffer[0], 0, "nothing was written");
    }

    /// The name arrives NUL-terminated inside the buffer it was given.
    ///
    /// The buffer is smaller than the default name, so the truncating path is the one under test.
    #[test]
    fn a_name_is_written_terminated_and_never_past_the_size_it_was_given() {
        // The default user is "player": four bytes of room holds three characters and a terminator.
        let mut buffer = [0xAA_u8; 16];
        let answered = super::user_service_get_user_name(&name_call(1, &mut buffer, 4));

        assert_eq!(answered, super::OK);
        assert_eq!(&buffer[..3], b"pla");
        assert_eq!(buffer[3], 0, "terminated inside the claimed size");
        assert_eq!(
            buffer[4], 0xAA,
            "and nothing beyond the claimed size was touched"
        );
    }

    #[test]
    fn every_implementation_is_also_declared() {
        // Every implementation is declared in one of the crate's modules, or resolution never
        // reaches it.
        let declared: Vec<&str> = super::MODULE
            .imports
            .iter()
            .chain(super::user::MODULE.imports.iter())
            .chain(super::sysmodule::MODULE.imports.iter())
            .chain(super::error_dialog::MODULE.imports.iter())
            .map(|i| i.name)
            .collect();
        for (name, _) in implementations() {
            assert!(
                declared.contains(name),
                "{name} is implemented but not declared in guest_module!"
            );
        }
    }

    /// The error-dialog init answers `OK` and reads none of its arguments.
    #[test]
    fn error_dialog_initialize_answers_ok() {
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::error_dialog_initialize(&args), super::OK);
    }

    /// The splash and flag actions answer `OK` without reading any argument slot.
    #[test]
    fn the_system_service_actions_succeed_without_touching_their_arguments() {
        let args = [0xDEAD_BEEF_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::hide_splash_screen(&args), super::OK);
        assert_eq!(
            super::disable_notice_screen_skip_flag_auto_set(&args),
            super::OK
        );
    }

    #[test]
    fn the_destination_is_always_written() {
        // The destination is overwritten, so the guest never reads stale stack.
        let mut value: u32 = 0xDEAD_BEEF;
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = std::ptr::addr_of_mut!(value) as usize as u64;

        assert_eq!(param_get_int(&args), 0, "reports success");
        assert_eq!(
            u64::from(value),
            UNKNOWN_PARAMETER,
            "and the destination no longer holds what was there before"
        );
    }

    #[test]
    fn a_request_with_nowhere_to_answer_is_refused() {
        // A null destination is refused instead of faulting inside the emulator.
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_ne!(param_get_int(&args), 0);
    }

    #[test]
    fn only_four_bytes_are_written() {
        // The interface answers an `int`; the neighbouring word is untouched.
        let mut pair: [u32; 2] = [0xAAAA_AAAA, 0xBBBB_BBBB];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[1] = std::ptr::addr_of_mut!(pair[0]) as usize as u64;

        assert_eq!(param_get_int(&args), 0);
        assert_eq!(u64::from(pair[0]), UNKNOWN_PARAMETER);
        assert_eq!(pair[1], 0xBBBB_BBBB, "the neighbour is untouched");
    }
}
