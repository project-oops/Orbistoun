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
pub mod random;
pub mod remoteplay;
pub mod rtc;
pub mod save_data;
pub mod web_browser_dialog;

pub mod console;

guest_module! {
    "libSceSystemService" {
        "sceSystemServiceParamGetInt" => 2,
        "sceSystemServiceHideSplashScreen" => 0,
        "sceSystemServiceGetStatus" => 1,
        "sceSystemServiceReceiveEvent" => 1,
        "sceSystemServiceGetHdrToneMapLuminance" => 1,
        // title id, argv, parameter block.
        "sceSystemServiceLaunchApp" => 3,
        // A system-flag setter. The real signature is unmeasured, so the arity is the trampoline's
        // six; the handler reads none of it.
        "sceSystemServiceDisableNoticeScreenSkipFlagAutoSet" => 6,
        "sceSystemServiceGetAppIdOfBigApp" => 0,
        "sceSystemServiceGetMainAppTitleId" => 2,
        "sceSystemServiceIsAppSuspended" => 1,
        "sceSystemServiceKillApp" => 1,
        "sceSystemServiceNavigateToGoHome" => 0,
        "sceSystemServicePowerTick" => 0,
        "sceSysUtilSendSystemNotificationWithText" => 2,
        "sceSystemServiceLaunchWebBrowser" => 2,
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
            // An address, flags, and the block to fill: libkernel's unwind query (D778).
            "sceSysmoduleGetModuleInfoForUnwind" => 3,
        }
    }
}

/// Application installation utility - `libSceAppInstUtil`.
pub mod app_inst_util {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceAppInstUtil" {
            "sceAppInstUtilInitialize" => 1,
            "sceAppInstUtilTerminate" => 0,
            "sceAppInstUtilAppInstallAll" => 3,
            "Wudg3Xe3heE" => 3,
        }
    }
}

/// Application launch and lifecycle utility - `libSceLncUtil`.
pub mod lnc_util {
    use orbistoun_hle::guest_module;

    guest_module! {
        "libSceLncUtil" {
            "sceLncUtilGetAppIdOfRunningBigApp" => 0,
            "sceLncUtilGetAppTitleId" => 2,
            "sceLncUtilSuspendApp" => 1,
            "sceLncUtilKillApp" => 1,
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

fn get_app_id_of_big_app(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn get_main_app_title_id(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    const TITLE_ID: &core::ffi::CStr = c"CUSA00000";
    let bytes = TITLE_ID.to_bytes_with_nul();
    if args[0] != 0 && args[1] >= bytes.len() as u64 {
        // SAFETY: writes null-terminated dummy title ID into guest buffer
        unsafe {
            let ptr = args[0] as *mut u8;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        }
    }
    OK
}

fn is_app_suspended(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn kill_app(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn navigate_to_go_home(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn power_tick(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn send_system_notification_with_text(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn app_inst_util_initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn app_inst_util_terminate(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn app_inst_util_app_install_all(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn launch_web_browser(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn lnc_util_get_app_id_of_running_big_app(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // 0xFFFF_FFFF (-1) indicates no foreground running big app
    0xFFFF_FFFF
}

fn lnc_util_get_app_title_id(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let buf = args[1];
    if buf != 0 {
        // SAFETY: writes null terminator into destination buffer if non-null
        unsafe {
            let ptr = buf as *mut u8;
            *ptr = 0;
        }
    }
    OK
}

fn lnc_util_suspend_app(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

fn lnc_util_kill_app(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
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

/// `SCE_USER_SERVICE_ERROR_NO_EVENT`, what the same call answers once its event has been given
/// (call-reuse).
const USER_SERVICE_NO_EVENT: u64 = 0x8096_0007;

/// Whether the signed-in user's login event has been given.
static LOGIN_EVENT_GIVEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `sceUserServiceGetEvent(event)` - the next user event.
///
/// As obSCEne's census measured it (`200-census/libSceUserService/sceUserServiceGetEvent`, sweep
/// 20261007-113500): the first call answers `0` with eight bytes, type `0` then the signed-in
/// user's identifier; the next answers `SCE_USER_SERVICE_ERROR_NO_EVENT` and writes nothing; a
/// null destination is `SCE_USER_SERVICE_ERROR_INVALID_ARGUMENT`. So a title learns its user has
/// logged in once, as on the console.
fn user_service_get_event(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(at) = usize::try_from(args[0]) else {
        return USER_SERVICE_INVALID_ARGUMENT;
    };
    if at == 0 {
        return USER_SERVICE_INVALID_ARGUMENT;
    }
    if LOGIN_EVENT_GIVEN.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return USER_SERVICE_NO_EVENT;
    }
    let mut event = [0_u8; 8];
    event[4..].copy_from_slice(&signed_in_user().to_le_bytes());
    // SAFETY: a guest-supplied event destination in identity-mapped guest memory, written at the
    // eight bytes the call was measured writing.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<[u8; 8]>(at), event);
    }
    OK
}

/// The bytes `sceSystemServiceGetStatus` writes: 136, all zero on the measured console
/// (`200-census/libSceSystemService/sceSystemServiceGetStatus`, extent and changed 136).
const SYSTEM_STATUS_BYTES: usize = 136;

/// `sceSystemServiceGetStatus(status)` - the system's state as the title sees it: no event pending,
/// nothing overlaid, in the foreground. Answers `0` with 136 zero bytes, as measured; a null
/// destination is refused, where the console faults.
fn get_status(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(at) = usize::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    if at == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    // SAFETY: a guest-supplied status destination in identity-mapped guest memory, written at the
    // 136 bytes the call was measured writing.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<[u8; SYSTEM_STATUS_BYTES]>(at),
            [0; SYSTEM_STATUS_BYTES],
        );
    }
    OK
}

/// `SCE_SYSTEM_SERVICE_ERROR_NO_EVENT`: what `sceSystemServiceReceiveEvent` answers with nothing
/// pending (`200-census/libSceSystemService/sceSystemServiceReceiveEvent`, sweep 20261008-004521).
const NO_SYSTEM_EVENT: u64 = 0x80a1_0004;

/// `sceSystemServiceReceiveEvent(event)` - the next system event, if one is pending. None is here,
/// as none was on the measured console: three calls in a row each answered
/// [`NO_SYSTEM_EVENT`] and left all 0x200 poisoned bytes of the buffer as they were.
fn receive_event(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NO_SYSTEM_EVENT
}

/// The twelve bytes `sceSystemServiceGetHdrToneMapLuminance` wrote on the measured console
/// (`200-census/libSceSystemService/sceSystemServiceGetHdrToneMapLuminance`, sweep 20261007-113500):
/// three floats, about 637.3, 981.1 and 0.125 - two peak luminances and a minimum, in nits.
const HDR_TONE_MAP_LUMINANCE: [u8; 12] = [
    0x6a, 0x54, 0x1f, 0x44, 0x58, 0x49, 0x75, 0x44, 0xec, 0xdd, 0xff, 0x3d,
];
/// What the same call answered for a null record (call-zeros).
const HDR_TONE_MAP_NO_RECORD: u64 = 0x80a1_0003;

/// `sceSystemServiceGetHdrToneMapLuminance(record)` - the display's tone-mapping luminances.
/// Answers `0` with the twelve bytes the measured console wrote, and `0x80a10003` for a null
/// record, as measured. orbistoun presents that console's display.
fn get_hdr_tone_map_luminance(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let Ok(at) = usize::try_from(args[0]) else {
        return HDR_TONE_MAP_NO_RECORD;
    };
    if at == 0 {
        return HDR_TONE_MAP_NO_RECORD;
    }
    // SAFETY: a guest-supplied record in identity-mapped guest memory, written at the twelve
    // bytes the call was measured writing.
    unsafe {
        std::ptr::write_unaligned(
            std::ptr::with_exposed_provenance_mut::<[u8; 12]>(at),
            HDR_TONE_MAP_LUMINANCE,
        );
    }
    OK
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

/// `SCE_USER_SERVICE_ERROR_INVALID_ARGUMENT`, measured for a wrong record size or no record.
const USER_SERVICE_INVALID_ARGUMENT: u64 = 0x8096_0005;
/// `SCE_USER_SERVICE_ERROR_INVALID_USER_ID`, measured for user ids -1 and 0.
const USER_SERVICE_INVALID_USER: u64 = 0x8096_0105;
/// The only record size `sceUserServiceGetGamePresets` accepts, held in the record's first quadword.
const GAME_PRESETS_BYTES: u64 = 0x30;

/// `sceUserServiceGetGamePresets(user, presets)`: for the signed-in user and a record whose first
/// quadword is its size, `0x30`, zeroes the other 40 bytes and answers `0` - the console's user had
/// no presets set, and every field read back zero. A size of 0 or `0x40`, or no record, answers
/// `0x80960005`; users -1 and 0 `0x80960105`; each writes nothing (obSCEne `-9a3e`, sweep
/// 20260927-223648, `130-layout/user-game-presets`). Another user than the signed-in one is taken
/// as invalid as those two were; a bad user with a bad record was not measured, and the user is
/// checked first.
fn user_service_get_game_presets(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (user, record) = (args[0], args[1]);
    if user != u64::from(signed_in_user()) {
        return USER_SERVICE_INVALID_USER;
    }
    if record == 0 {
        return USER_SERVICE_INVALID_ARGUMENT;
    }
    // SAFETY: the guest's record, whose first quadword is its size by the call's contract.
    if unsafe { orbistoun_mem::guest::read_u64(record) } != Some(GAME_PRESETS_BYTES) {
        return USER_SERVICE_INVALID_ARGUMENT;
    }
    // SAFETY: the same record, 0x30 bytes as its size field says.
    if unsafe { orbistoun_mem::guest::write_bytes(record + 8, &[0; 40]) } {
        OK
    } else {
        USER_SERVICE_INVALID_ARGUMENT
    }
}

/// What `sceUserServiceGetAgeLevel` answers for the signed-in user. Measured; its meaning is not
/// established.
const AGE_LEVEL_SIGNED_IN: u64 = 0x8096_0002;
/// What it answers for a value that is not a user: 0 and a buffer's address, the census's zero and
/// buffer arguments.
const AGE_LEVEL_NOT_A_USER: u64 = 0x8096_0009;

/// `sceUserServiceGetAgeLevel(user, out)`: for the signed-in user answers `0x80960002` and writes
/// none of `out` (obSCEne `070-user/oneshot-setup-calls` arm 3, a 0x40-byte buffer filled with
/// `0xa5`, `20261009-104652-eboot.obs.log`); for anything else `0x80960009`, as the census's zero
/// and buffer arguments were answered. Another user than the signed-in one is taken as not a user,
/// as [`user_service_get_game_presets`] takes it.
fn user_service_get_age_level(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == u64::from(signed_in_user()) {
        AGE_LEVEL_SIGNED_IN
    } else {
        AGE_LEVEL_NOT_A_USER
    }
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
        (
            "_ZN3sce4Json11InitializerC1Ev",
            json2::initializer_construct,
        ),
        (
            "_ZN3sce4Json11Initializer10initializeEPKNS0_13InitParameterE",
            json2::initializer_initialize,
        ),
        (
            "_ZN3sce4Json11Initializer9terminateEv",
            json2::initializer_terminate,
        ),
        (
            "_ZN3sce4Json12MemAllocatorC2Ev",
            json2::mem_allocator_construct,
        ),
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
        ("sceUserServiceGetAgeLevel", user_service_get_age_level),
        (
            "sceUserServiceGetGamePresets",
            user_service_get_game_presets,
        ),
        ("sceUserServiceGetEvent", user_service_get_event),
        ("sceSystemServiceGetStatus", get_status),
        ("sceSystemServiceReceiveEvent", receive_event),
        (
            "sceSystemServiceGetHdrToneMapLuminance",
            get_hdr_tone_map_luminance,
        ),
        ("sceErrorDialogInitialize", error_dialog_initialize),
        ("sceSystemServiceParamGetInt", param_get_int),
        ("sceSystemServiceHideSplashScreen", hide_splash_screen),
        (
            "sceSystemServiceDisableNoticeScreenSkipFlagAutoSet",
            disable_notice_screen_skip_flag_auto_set,
        ),
        ("sceSystemServiceGetAppIdOfBigApp", get_app_id_of_big_app),
        ("sceSystemServiceGetMainAppTitleId", get_main_app_title_id),
        ("sceSystemServiceIsAppSuspended", is_app_suspended),
        ("sceSystemServiceKillApp", kill_app),
        ("sceSystemServiceNavigateToGoHome", navigate_to_go_home),
        ("sceSystemServicePowerTick", power_tick),
        (
            "sceSysUtilSendSystemNotificationWithText",
            send_system_notification_with_text,
        ),
        ("sceSysmoduleLoadModule", sysmodule_load_module),
        (
            "sceSysmoduleGetModuleInfoForUnwind",
            orbistoun_kernel::get_module_info_for_unwind,
        ),
        ("sceSysmoduleUnloadModule", sysmodule_unload_module),
        ("sceSysmoduleIsLoaded", sysmodule_is_loaded),
        ("sceAppInstUtilInitialize", app_inst_util_initialize),
        ("sceAppInstUtilTerminate", app_inst_util_terminate),
        ("sceAppInstUtilAppInstallAll", app_inst_util_app_install_all),
        ("Wudg3Xe3heE", app_inst_util_app_install_all),
        ("sceSystemServiceLaunchWebBrowser", launch_web_browser),
        (
            "sceLncUtilGetAppIdOfRunningBigApp",
            lnc_util_get_app_id_of_running_big_app,
        ),
        ("sceLncUtilGetAppTitleId", lnc_util_get_app_title_id),
        ("sceLncUtilSuspendApp", lnc_util_suspend_app),
        ("sceLncUtilKillApp", lnc_util_kill_app),
        ("sceSaveDataInitialize3", save_data::initialize3),
        (
            "sceSaveDataSetupSaveDataMemory2",
            save_data::setup_save_data_memory2,
        ),
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
            .chain(super::app_inst_util::MODULE.imports.iter())
            .chain(super::lnc_util::MODULE.imports.iter())
            .chain(super::error_dialog::MODULE.imports.iter())
            .chain(super::save_data::MODULE.imports.iter())
            .chain(super::json2::MODULE.imports.iter())
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

    #[test]
    fn app_inst_util_actions_succeed() {
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::app_inst_util_initialize(&args), super::OK);
        assert_eq!(super::app_inst_util_terminate(&args), super::OK);
        assert_eq!(super::app_inst_util_app_install_all(&args), super::OK);
    }

    #[test]
    fn lnc_util_actions_succeed() {
        let args = [0_u64; GUEST_ARG_REGISTERS];
        assert_eq!(super::launch_web_browser(&args), super::OK);
        assert_eq!(
            super::lnc_util_get_app_id_of_running_big_app(&args),
            0xFFFF_FFFF
        );
        assert_eq!(super::lnc_util_get_app_title_id(&args), super::OK);
        assert_eq!(super::lnc_util_suspend_app(&args), super::OK);
        assert_eq!(super::lnc_util_kill_app(&args), super::OK);
    }
}
