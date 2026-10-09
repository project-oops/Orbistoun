//! `libSceIme` - the on-screen text-entry service.
//!
//! Every name here is declared; the keyboard's open, close and update are implemented as
//! measured. Names are confirmed; most arities are not. An unmeasured arity is `6`, the
//! trampoline's full capture, because a wrong arity only degrades a trace while a wrong name is a
//! shim nothing can reach (D504).

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;

/// What `sceImeUpdate` answers with no keyboard open: `0x80bc0002`, measured with every argument
/// a poisoned buffer and with every argument zero, nothing written (obSCEne census,
/// `200-census/libSceIme/sceImeUpdate`, sweep 20261007-202010). orbistoun opens no keyboard.
const NO_KEYBOARD_OPEN: u64 = 0x80bc_0002;

/// `SCE_IME_ERROR_INVALID_USER_ID`, what opening for user `0xffffffff` answered.
const INVALID_USER: u64 = 0x80bc_0010;
/// `SCE_IME_ERROR_ALREADY_OPENED`, what a second open for the same user answered.
const ALREADY_OPENED: u64 = 0x80bc_0001;
/// The user `0xffffffff` (everyone), which the keyboard refuses.
const EVERYONE: u64 = 0xffff_ffff;

/// The users with a keyboard open.
fn opened() -> &'static std::sync::Mutex<Vec<u64>> {
    static OPENED: std::sync::OnceLock<std::sync::Mutex<Vec<u64>>> = std::sync::OnceLock::new();
    OPENED.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// `sceImeUpdate(handler)` - delivers pending keyboard events to the handler. There are none:
/// with a keyboard open it answers 0, as the console did with no key pressed, and with none open
/// `0x80bc0002` (REQ-ik01, `101-input-ext/ime-keyboard-lifecycle`).
pub(crate) fn update(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match opened().lock() {
        Ok(users) if !users.is_empty() => 0,
        _ => NO_KEYBOARD_OPEN,
    }
}

/// `sceImeKeyboardOpen(user, param)`: opens the keyboard for a user and answers 0, with no
/// keyboard attached as on the console; a second open for the same user answers `0x80bc0001`, and
/// user `0xffffffff` is refused with `0x80bc0010`, the parameter block untouched either way
/// (REQ-ik01, `20261009-151440-eboot.obs.log`). Any user other than everyone is taken as valid.
pub(crate) fn keyboard_open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let user = args[0] & 0xffff_ffff;
    if user == EVERYONE {
        return INVALID_USER;
    }
    let Ok(mut users) = opened().lock() else {
        return INVALID_USER;
    };
    if users.contains(&user) {
        return ALREADY_OPENED;
    }
    users.push(user);
    0
}

/// `sceImeKeyboardClose(user)`: closes a user's keyboard and answers 0, or `0x80bc0002` for a user
/// with none open (REQ-ik01).
pub(crate) fn keyboard_close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let user = args[0] & 0xffff_ffff;
    let Ok(mut users) = opened().lock() else {
        return NO_KEYBOARD_OPEN;
    };
    match users.iter().position(|&open| open == user) {
        Some(at) => {
            users.remove(at);
            0
        }
        None => NO_KEYBOARD_OPEN,
    }
}

guest_module! {
    "libSceIme" {
        // (user)
        "sceImeKeyboardClose" => 1,
        "sceImeKeyboardGetInfo" => 6,
        "sceImeKeyboardGetResourceId" => 6,
        // (user, param)
        "sceImeKeyboardOpen" => 2,
        "sceImeUpdate" => 6,
    }
}
