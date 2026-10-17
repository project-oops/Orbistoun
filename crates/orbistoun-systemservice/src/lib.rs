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
pub mod content_delete;
pub mod content_export;
pub mod coredump;
pub mod error_dialog;
pub mod json2;
pub mod launch;
pub mod msg_dialog;
pub mod offline;
pub mod play_go;
pub mod random;
pub mod remoteplay;
pub mod rtc;
pub mod save_data;
pub mod share;
pub mod signin_dialog;
pub mod trophy;
pub mod uds_stats;
pub mod web_browser_dialog;

pub mod console;

guest_module! {
    "libSceSystemService" {
        "sceSystemServiceParamGetInt" => 2,
        "sceSystemServiceHideSplashScreen" => 0,
        "sceSystemServiceGetStatus" => 1,
        // (flag): one byte.
        "sceSystemServiceGetNoticeScreenSkipFlag" => 1,
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
            // (user): there is none in the foreground, as measured.
            "sceUserServiceGetForegroundUser" => 1,
            "sceUserServiceGetGamePresets" => 2,
            "sceUserServiceGetEvent" => 1,
            "sceUserServiceGetAccessibilityVibration" => 2,
            "sceUserServiceGetAccessibilityTriggerEffect" => 2,
            "sceUserServiceGetAccessibilityPressAndHoldDelay" => 2,
            "sceUserServiceGetAccessibilityChatTranscription" => 2,
            "sceUserServiceGetAccessibilityZoomEnabled" => 2,
            "sceUserServiceGetAccessibilityZoomFollowFocus" => 2,
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
            "0xe185b777dd60e75a" => 3,
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
    /// What a console answered for a null out-pointer (obSCEne REQ-pg02,
    /// `20261010-231900-eboot.obs.log` 3070), the code it gave every refused identifier too.
    const REFUSED: u64 = 0x80a1_0003;
    let out = args[1];
    if out == 0 {
        return REFUSED;
    }
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
#[must_use]
pub fn signed_in_user() -> u32 {
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

/// `sceUserServiceGetLoginUserIdList(out)` - which users are signed in: four identifiers, the
/// signed-in user and three `-1`, as a console wrote them for one signed-in user.
fn user_service_get_login_user_id_list(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    let Ok(at) = usize::try_from(args[0]) else {
        return u64::from(GuestError::InvalidArgument.as_raw());
    };
    // Four identifiers, the unused ones `-1`, as measured for one signed-in user
    // (`20261011-1791715554-eboot.obs.log` 1930).
    let list = [signed_in_user(), u32::MAX, u32::MAX, u32::MAX];
    // SAFETY: a guest-supplied list in identity-mapped guest memory, written at the sixteen bytes
    // the console was measured writing.
    unsafe {
        std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<[u32; 4]>(at), list);
    }
    OK
}

/// `SCE_USER_SERVICE_ERROR_INVALID_ARGUMENT`, measured for a wrong record size or no record.
const USER_SERVICE_INVALID_ARGUMENT: u64 = 0x8096_0005;
/// `SCE_USER_SERVICE_ERROR_INVALID_USER_ID`, measured for user ids -1 and 0.
const USER_SERVICE_INVALID_USER: u64 = 0x8096_0105;
/// The record sizes `sceUserServiceGetGamePresets` accepts, held in the record's first quadword:
/// `0x30` (`-9a3e`) and `0x28`, the size Unity passes (REQ-us01, `get-game-presets`).
const GAME_PRESETS_SIZES: [u64; 2] = [0x28, 0x30];

/// `sceUserServiceGetGamePresets(user, presets)`: for the signed-in user and a record whose first
/// quadword is its size, `0x30` or `0x28`, zeroes the rest of the record and answers `0` - the
/// console's user had no presets set, and every field read back zero (a `0x28` record answered `0`
/// with its bytes, already zero, unchanged; REQ-us01). A size of 0 or `0x40`, or no record, answers
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
    let size = unsafe { orbistoun_mem::guest::read_u64(record) };
    let Some(size) = size.filter(|size| GAME_PRESETS_SIZES.contains(size)) else {
        return USER_SERVICE_INVALID_ARGUMENT;
    };
    let zeroes = vec![0_u8; usize::try_from(size - 8).unwrap_or(0)];
    // SAFETY: the same record, as many bytes as its size field says.
    if unsafe { orbistoun_mem::guest::write_bytes(record + 8, &zeroes) } {
        OK
    } else {
        USER_SERVICE_INVALID_ARGUMENT
    }
}

/// The age level `sceUserServiceGetAgeLevel` writes for the signed-in user: 21, as a console's
/// signed-in user reported it in a fresh process after `sceUserServiceInitialize`
/// (`000-boot/unity-user-service-chain`, `get-age-level`, `report-1791758178.txt`). A user's own
/// setting there; orbistoun's users carry none, so each reports the measured one.
const AGE_LEVEL: u32 = 0x15;
/// What the user-service getters answer for a value that is not a user (`get-age-level-user-0xff`,
/// and the census's zero and buffer arguments): `SCE_USER_SERVICE_ERROR_NO_USER`, nothing written.
const AGE_LEVEL_NOT_A_USER: u64 = 0x8096_0009;
/// The accessibility settings the signed-in user reported in the same chain, each written as a
/// word: chat transcription and press-and-hold delay `0`, trigger effect and vibration `1`.
/// Zoom `0` too, measured later (REQ-ag01, `000-boot/ag01-aux-calls`, `report-1791801495.txt`
/// 433-434). Zoom follow-focus `1` (REQ-8b13, `000-boot/user-service-zoom-follow-focus`,
/// `report-1791833977.txt` 724-734: `01000000` then the slot's own bytes).
const ACCESSIBILITY: [(&str, u32); 6] = [
    ("sceUserServiceGetAccessibilityChatTranscription", 0),
    ("sceUserServiceGetAccessibilityPressAndHoldDelay", 0),
    ("sceUserServiceGetAccessibilityTriggerEffect", 1),
    ("sceUserServiceGetAccessibilityVibration", 1),
    ("sceUserServiceGetAccessibilityZoomEnabled", 0),
    ("sceUserServiceGetAccessibilityZoomFollowFocus", 1),
];

/// `sceUserServiceGetAgeLevel(user, out)`: for the signed-in user answers `0x80960002` and writes
/// none of `out` (obSCEne `070-user/oneshot-setup-calls` arm 3, a 0x40-byte buffer filled with
/// `0xa5`, `20261009-104652-eboot.obs.log`); for anything else `0x80960009`, as the census's zero
/// and buffer arguments were answered. Another user than the signed-in one is taken as not a user,
/// as [`user_service_get_game_presets`] takes it.
fn user_service_get_age_level(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    write_user_word(args, AGE_LEVEL)
}

/// For the signed-in user, writes `value` to the word at `args[1]` and answers 0, as each getter in
/// `000-boot/unity-user-service-chain` did; any other value answers [`AGE_LEVEL_NOT_A_USER`] with
/// nothing written, and a null out pointer is refused.
fn write_user_word(args: &[u64; GUEST_ARG_REGISTERS], value: u32) -> u64 {
    if args[0] != u64::from(signed_in_user()) {
        return AGE_LEVEL_NOT_A_USER;
    }
    if args[1] == 0 {
        return USER_SERVICE_INVALID_ARGUMENT;
    }
    // SAFETY: the guest's out word, by the call's contract; written through the checked accessor.
    if unsafe { orbistoun_mem::guest::write_u32(args[1], value) } {
        OK
    } else {
        USER_SERVICE_INVALID_ARGUMENT
    }
}

/// `sceUserServiceGetForegroundUser(&user)`: `0x80960009` with the slot left as it was, though a
/// user was signed in, and `0x80960005` for a null slot (`200-census/libSceUserService/
/// sceUserServiceGetForegroundUser`, `20261009-151440-eboot.obs.log`).
fn user_service_get_foreground_user(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        USER_SERVICE_INVALID_ARGUMENT
    } else {
        AGE_LEVEL_NOT_A_USER
    }
}

/// `sceUserServiceGetAccessibility*(user, out)`: the setting [`ACCESSIBILITY`] names for the
/// getter, written as [`write_user_word`] writes.
fn accessibility(name: &str, args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let value = ACCESSIBILITY
        .iter()
        .find(|(getter, _)| *getter == name)
        .map_or(0, |&(_, value)| value);
    write_user_word(args, value)
}

fn accessibility_chat_transcription(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    accessibility(ACCESSIBILITY[0].0, args)
}

fn accessibility_press_and_hold_delay(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    accessibility(ACCESSIBILITY[1].0, args)
}

fn accessibility_trigger_effect(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    accessibility(ACCESSIBILITY[2].0, args)
}

fn accessibility_vibration(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    accessibility(ACCESSIBILITY[3].0, args)
}

fn accessibility_zoom_enabled(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    accessibility(ACCESSIBILITY[4].0, args)
}

fn accessibility_zoom_follow_focus(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    accessibility(ACCESSIBILITY[5].0, args)
}

/// `sceSystemServiceGetNoticeScreenSkipFlag(flag)`: writes one byte, the system's skip flag for
/// this title, and answers 0 (REQ-cn10 arm 4: of a buffer filled `0xa5`, only its first byte
/// changed). The system holds the flag per title: 0 at a first launch, set once a launch has run
/// 120 seconds with auto-set on (the console's own `NoticeScreenSkipGetFlag` lines read 0 for a
/// new probe and 1 for one run before, with a 120-second timer started at launch). A run is a
/// first launch, so 0 - the 1 cn10 read came from a probe launched many times. A null flag is
/// unmeasured and refused with the placeholder.
fn system_service_get_notice_screen_skip_flag(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's out-parameter, one byte by the measurement.
    if args[0] != 0 && unsafe { orbistoun_mem::guest::write_bytes(args[0], &[0]) } {
        OK
    } else {
        u64::from(GuestError::InvalidArgument.as_raw())
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

/// What the sysmodule calls answer, as a console answered them in a fresh process (obSCEne
/// `REQ-20261013T1600Z-sm41`, `000-boot/sysmodule-isloaded-lifecycle`,
/// `reports/hardware/report-20261013-172400.log` lines 1950-1990, FW 12.40).
///
/// Every library a title imports is resolved before the guest runs, so a load here pages nothing
/// in (D125); what the calls track is whether the guest has asked. Identifiers no measurement
/// covers keep D125's answer, loaded, so they move nothing.
mod sysmodule_state {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    /// `SCE_SYSMODULE_ERROR_INVALID_ID`: an identifier that names no module, from every call.
    pub(super) const INVALID_ID: u64 = 0x805a_1000;
    /// `SCE_SYSMODULE_ERROR_UNLOADED`: a module that exists and is not loaded.
    pub(super) const UNLOADED: u64 = 0x805a_1001;

    /// Identifiers every call refused with [`INVALID_ID`].
    const INVALID: [u64; 7] = [0, 0x08, 0x0e, 0x15, 0x24, 0x80, 0x7fff];
    /// Modules loaded before the process asked, which stay loaded through an unload.
    const PRELOADED: [u64; 2] = [0x07, 0x96];
    /// Modules that answered [`UNLOADED`] until loaded, and again after an unload. 0xb4 is the one
    /// PPSA04263 loads, and its own init with it, only on that answer.
    const ON_REQUEST: [u64; 3] = [0x1b, 0x1c, 0xb4];

    /// The [`ON_REQUEST`] modules the guest has loaded and not unloaded.
    fn loaded() -> &'static Mutex<BTreeSet<u64>> {
        static LOADED: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());
        &LOADED
    }

    pub(super) fn is_invalid(id: u64) -> bool {
        INVALID.contains(&id)
    }

    /// Whether `id` is loaded: [`ON_REQUEST`] modules only once the guest loaded them.
    pub(super) fn is_loaded(id: u64) -> bool {
        !ON_REQUEST.contains(&id)
            || loaded()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&id)
    }

    pub(super) fn load(id: u64) {
        if ON_REQUEST.contains(&id) {
            loaded()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(id);
        }
    }

    /// Unloads `id`; a [`PRELOADED`] module stays loaded, as it did on the console.
    pub(super) fn unload(id: u64) {
        if !PRELOADED.contains(&id) {
            loaded()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
        }
    }
}

/// `sceSysmoduleLoadModule(id)` - brings a system module in so its functions can be called.
///
/// Answers `0` and records the load; an identifier that names no module answers `0x805a1000`
/// (measured). A positive placeholder would read as a module handle the guest might dereference.
fn sysmodule_load_module(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let id = args[0] & 0xffff_ffff;
    if sysmodule_state::is_invalid(id) {
        return sysmodule_state::INVALID_ID;
    }
    sysmodule_state::load(id);
    OK
}

/// `sceSysmoduleUnloadModule(id)` - answers `0` and records the unload, or `0x805a1000` for an
/// identifier that names no module (measured).
fn sysmodule_unload_module(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let id = args[0] & 0xffff_ffff;
    if sysmodule_state::is_invalid(id) {
        return sysmodule_state::INVALID_ID;
    }
    sysmodule_state::unload(id);
    OK
}

/// `sceSysmoduleIsLoaded(id)` - `0` for a loaded module, `0x805a1001` for one not loaded, and
/// `0x805a1000` for an identifier that names no module (measured; see [`sysmodule_state`]).
fn sysmodule_is_loaded(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let id = args[0] & 0xffff_ffff;
    if sysmodule_state::is_invalid(id) {
        sysmodule_state::INVALID_ID
    } else if sysmodule_state::is_loaded(id) {
        OK
    } else {
        sysmodule_state::UNLOADED
    }
}

/// Implementations this crate provides, by symbol name. Names rather than hashes, so the table can
/// be read and checked against the declarations.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    IMPLEMENTATIONS
}

/// The list [`implementations`] answers.
const IMPLEMENTATIONS: &[(&str, GuestFn)] = &[
    (
        "_ZN3sce4Json11InitializerC1Ev",
        json2::initializer_construct,
    ),
    (
        "_ZN3sce4Json11Initializer10initializeEPKNS0_13InitParameterE",
        json2::initializer_initialize,
    ),
    (
        "_ZN3sce4Json11Initializer10initializeEPKNS0_14InitParameter2E",
        json2::initializer_initialize,
    ),
    (
        "_ZN3sce4Json14InitParameter2C1Ev",
        json2::init_parameter2_construct,
    ),
    (
        "_ZN3sce4Json14InitParameter212setAllocatorEPNS0_12MemAllocatorEPv",
        json2::init_parameter2_set_allocator,
    ),
    (
        "_ZN3sce4Json14InitParameter217setFileBufferSizeEm",
        json2::init_parameter2_set_file_buffer_size,
    ),
    (
        "_ZN3sce4Json11Initializer9terminateEv",
        json2::initializer_terminate,
    ),
    (
        "_ZN3sce4Json12MemAllocatorC2Ev",
        json2::mem_allocator_construct,
    ),
    ("_ZN3sce4Json5ValueC1Ev", json2::value_construct),
    ("_ZN3sce4Json5ValueD1Ev", json2::value_destruct),
    (
        "_ZN3sce4Json5Value3setENS0_9ValueTypeE",
        json2::value_set_type,
    ),
    ("_ZN3sce4Json5Value3setEb", json2::value_set_bool),
    ("_ZN3sce4Json5Value3setEl", json2::value_set_integer),
    ("_ZN3sce4Json5Value3setEm", json2::value_set_unsigned),
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
        "sceUserServiceGetForegroundUser",
        user_service_get_foreground_user,
    ),
    (
        "sceUserServiceGetAccessibilityVibration",
        accessibility_vibration,
    ),
    (
        "sceUserServiceGetAccessibilityTriggerEffect",
        accessibility_trigger_effect,
    ),
    (
        "sceUserServiceGetAccessibilityChatTranscription",
        accessibility_chat_transcription,
    ),
    (
        "sceUserServiceGetAccessibilityPressAndHoldDelay",
        accessibility_press_and_hold_delay,
    ),
    (
        "sceUserServiceGetAccessibilityZoomEnabled",
        accessibility_zoom_enabled,
    ),
    (
        "sceUserServiceGetAccessibilityZoomFollowFocus",
        accessibility_zoom_follow_focus,
    ),
    (
        "sceSystemServiceGetNoticeScreenSkipFlag",
        system_service_get_notice_screen_skip_flag,
    ),
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
    ("sceErrorDialogInitialize", error_dialog::initialize),
    ("sceErrorDialogOpen", error_dialog::open),
    ("sceErrorDialogGetStatus", error_dialog::get_status),
    ("sceErrorDialogUpdateStatus", error_dialog::update_status),
    ("sceErrorDialogClose", error_dialog::close),
    ("sceErrorDialogTerminate", error_dialog::terminate),
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
    ("0xe185b777dd60e75a", app_inst_util_app_install_all),
    ("sceSystemServiceLaunchWebBrowser", launch_web_browser),
    (
        "sceLncUtilGetAppIdOfRunningBigApp",
        lnc_util_get_app_id_of_running_big_app,
    ),
    ("sceLncUtilGetAppTitleId", lnc_util_get_app_title_id),
    ("sceLncUtilSuspendApp", lnc_util_suspend_app),
    ("sceLncUtilKillApp", lnc_util_kill_app),
    ("sceSaveDataInitialize3", save_data::initialize3),
    ("sceSaveDataMount3", save_data::mount3),
    ("sceSaveDataUmount2", save_data::umount2),
    ("sceSaveDataDirNameSearch", save_data::dir_name_search),
    ("sceSaveDataDelete", save_data::delete),
    (
        "sceSaveDataCreateTransactionResource",
        save_data::create_transaction_resource,
    ),
    ("sceSaveDataPrepare", save_data::prepare),
    ("sceSaveDataCommit", save_data::commit),
    (
        "sceSaveDataDeleteTransactionResource",
        save_data::delete_transaction_resource,
    ),
    (
        "sceSaveDataGetSaveDataMemory2",
        save_data::get_save_data_memory2,
    ),
    (
        "sceSaveDataSetupSaveDataMemory2",
        save_data::setup_save_data_memory2,
    ),
    (
        "sceSaveDataSetSaveDataMemory2",
        save_data::set_save_data_memory2,
    ),
    (
        "sceSaveDataSyncSaveDataMemory",
        save_data::sync_save_data_memory,
    ),
];

#[cfg(test)]
mod tests {
    use super::{MAX_USER_NAME, UNKNOWN_PARAMETER, implementations, param_get_int};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The NID-only installer entry links to the NID the guest imports: a name in `0x` hex is taken
    /// as the NID itself, where any other spelling is hashed as a symbol name and links to nothing
    /// called.
    #[test]
    fn the_installer_nid_links_to_the_nid_the_guest_imports() {
        let mut registry = orbistoun_hle::Registry::new(
            orbistoun_nid::NidHasher::new(*b"test-suffix"),
            orbistoun_hle::StubPolicy::default(),
        );
        registry.register(super::app_inst_util::MODULE);
        assert!(
            registry
                .resolve(orbistoun_nid::Nid::from_raw(0xe185_b777_dd60_e75a))
                .is_some()
        );
    }

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

    /// A run is the title's first launch, whose notice-screen skip flag the system holds at 0: it
    /// is set only after a launch has run its 120 seconds with auto-set left on, or by the title.
    /// GTA V, answered 1, skipped its intro video and legal pages, which the console shows it.
    #[test]
    fn the_notice_screen_skip_flag_is_clear_at_a_first_launch() {
        let mut flag = [0xa5_u8; 4];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = flag.as_mut_ptr() as usize as u64;
        assert_eq!(
            super::system_service_get_notice_screen_skip_flag(&args),
            super::OK
        );
        assert_eq!(flag, [0, 0xa5, 0xa5, 0xa5], "one byte, cleared");
    }

    /// The signed-in users are four identifiers, the unused ones `-1`, as a console wrote them for
    /// one signed-in user (`20261011-1791715554-eboot.obs.log` 1930, `130-layout/user-service`:
    /// `d9f4a21e ffffffff ffffffff ffffffff`).
    #[test]
    fn the_login_list_is_four_identifiers_the_unused_minus_one() {
        let mut list = [0_u32; 5];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = list.as_mut_ptr() as usize as u64;
        assert_eq!(super::user_service_get_login_user_id_list(&args), super::OK);
        assert_eq!(
            list,
            [super::signed_in_user(), u32::MAX, u32::MAX, u32::MAX, 0],
            "four written, nothing past them"
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
        // A null destination is refused with the code a console answered for id 1 (obSCEne
        // REQ-pg02, `20261010-231900-eboot.obs.log` 3070).
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = 1;
        assert_eq!(param_get_int(&args), 0x80a1_0003);
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
