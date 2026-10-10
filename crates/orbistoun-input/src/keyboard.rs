//! `libSceKeyboard` - keyboard input, read from the host's keyboard (D783).
//!
//! `sceKeyboardInit`, `sceKeyboardOpen`, `sceKeyboardReadState` and `sceKeyboardClose` answer as
//! obSCEne measured them (`100-input/keyboard-and-media-gates`, `101-input-ext/keyboard-lifecycle`
//! and the census, `20261009-151440-eboot.obs.log`). The keys a read reports are the host's, set
//! through [`set_held`] by whichever shell owns a keyboard; a run with none reports a connected
//! keyboard with nothing held. `sceKeyboardSetProcessPrivilege` and `sceKeyboardSetProcessFocus`
//! answer as `100-input/keyboard-privilege-focus` measured (`20261010-115300-eboot.log`).

use std::sync::Mutex;

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceKeyboard" {
        "sceKeyboardInit" => 0,
        // (user, type, index, param).
        "sceKeyboardOpen" => 4,
        // (handle, state).
        "sceKeyboardReadState" => 2,
        // (handle).
        "sceKeyboardClose" => 1,
        "sceKeyboardSetProcessPrivilege" => 1,
        "sceKeyboardSetProcessFocus" => 1,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceKeyboardInit", init),
        ("sceKeyboardOpen", open),
        ("sceKeyboardReadState", read_state),
        ("sceKeyboardClose", close),
        ("sceKeyboardSetProcessPrivilege", set_process_privilege),
        ("sceKeyboardSetProcessFocus", set_process_focus),
    ]
}

/// What an open for a user that is not signed in answered: user `-1`, and a buffer's address.
const NOT_A_USER: u64 = 0x809b_0001;
/// What an open with every argument zero answered.
const ZERO_ARGUMENTS: u64 = 0x809b_0081;
/// What a second open of an index the user already has open answered.
const ALREADY_OPEN: u64 = 0x80da_0004;
/// What a read or a close of handle `-1` answered.
const BAD_HANDLE: u64 = 0x80da_0003;
/// What `sceKeyboardSetProcessPrivilege` answered for 1 and 0, and `sceKeyboardSetProcessFocus`
/// for 1: `-1`, as a 32-bit return.
const REFUSED: u64 = 0xffff_ffff;
/// What `sceKeyboardSetProcessFocus(0)` answered.
const BAD_PARAMETER: u64 = 0x80da_0001;

/// How many keyboard indices a user can hold open: 0 and 1, both opened by oops-sdk on a console
/// (Craft's hardware capture of 2026-10-06, handles `22939392` and `23004929`). A higher index is
/// unmeasured.
const INDICES: usize = 2;

/// The handle an open of `index` hands out: small and positive, not an address (D151). The
/// console's were large and varied between runs (`0x16b0700`, `0x1720700`), so no value of them is
/// a contract.
const fn handle_for(index: usize) -> u64 {
    index as u64 + 1
}

/// Bytes of the state record a read writes, all of them, as measured.
const RECORD_BYTES: usize = 96;
/// Where the record says a keyboard is present (oops-sdk `keyboard.c`, read by SeaShell).
const CONNECTED_AT: usize = 0x10;
/// Where it holds how many keys are down.
const COUNT_AT: usize = 0x14;
/// Where the held keys start, as USB HID usage codes, two bytes each.
const KEYS_AT: usize = 0x20;
/// How many held keys the record has room for.
pub const MOST_KEYS: usize = 16;

/// Which indices the signed-in user has open.
static OPEN: Mutex<[bool; INDICES]> = Mutex::new([false; INDICES]);

/// The host keys held now, as USB HID usage codes.
static HELD: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// Records which host keys are held, as USB HID usage codes; at most [`MOST_KEYS`] are reported.
///
/// Called by a shell that owns the host's keyboard, as it reports pad state.
pub fn set_held(keys: &[u16]) {
    if let Ok(mut held) = HELD.lock() {
        held.clear();
        held.extend(keys.iter().copied().filter(|&k| k != 0).take(MOST_KEYS));
    }
}

/// `sceKeyboardInit()`: answers 0, as measured.
fn init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceKeyboardOpen(user, type, index, param)`: the signed-in user gets a handle for index 0 or 1;
/// a second open of an index already open `0x80da0004`; every argument zero `0x809b0081`; any
/// other user `0x809b0001`, as measured. A higher index is unmeasured and answers the placeholder.
fn open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args.iter().take(4).all(|&a| a == 0) {
        return ZERO_ARGUMENTS;
    }
    let user = args[0] as u32;
    if user != orbistoun_systemservice::signed_in_user() {
        return NOT_A_USER;
    }
    let Ok(index) = usize::try_from(args[2] as u32) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if index >= INDICES {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let Ok(mut open) = OPEN.lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if open[index] {
        return ALREADY_OPEN;
    }
    open[index] = true;
    handle_for(index)
}

/// The index `handle` was handed out for, if it is open.
fn open_index(handle: u64) -> Option<usize> {
    let index = (0..INDICES).find(|&i| handle_for(i) == u64::from(handle as u32))?;
    OPEN.lock().ok()?.get(index).copied()?.then_some(index)
}

/// The 96-byte record for the keys held now: present, how many keys are down, and which. With
/// nothing held the length is 1 and no key is listed, as both console reads at rest wrote
/// (`101-input-ext/keyboard-lifecycle`, `100-input/keyboard-privilege-focus`).
fn record(held: &[u16]) -> [u8; RECORD_BYTES] {
    let mut out = [0_u8; RECORD_BYTES];
    out[CONNECTED_AT] = 1;
    let count = held.len().min(MOST_KEYS);
    let length = count.max(1);
    out[COUNT_AT..COUNT_AT + 4].copy_from_slice(&(length as u32).to_le_bytes());
    for (i, key) in held.iter().take(count).enumerate() {
        out[KEYS_AT + i * 2..KEYS_AT + i * 2 + 2].copy_from_slice(&key.to_le_bytes());
    }
    out
}

/// `sceKeyboardReadState(handle, state)`: writes all 96 bytes of the record for the host keys held
/// and answers 0; handle `-1` answers `0x80da0003`, as measured. Any other handle, and a state that
/// cannot be written, are unmeasured and answer the placeholder.
fn read_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (handle, state) = (args[0], args[1]);
    if handle as u32 == u32::MAX {
        return BAD_HANDLE;
    }
    if open_index(handle).is_none() {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let held = HELD.lock().map(|h| h.clone()).unwrap_or_default();
    // SAFETY: the guest's state record, 96 bytes by the call's contract.
    if unsafe { guest::write_bytes(state, &record(&held)) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceKeyboardSetProcessPrivilege(privilege)`: `-1` for 1 and for 0, as measured; any other value
/// is unmeasured.
fn set_process_privilege(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match args[0] as u32 {
        0 | 1 => REFUSED,
        _ => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// `sceKeyboardSetProcessFocus(focus)`: `-1` for 1 and `0x80da0001` for 0, as measured; any other
/// value is unmeasured.
fn set_process_focus(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    match args[0] as u32 {
        1 => REFUSED,
        0 => BAD_PARAMETER,
        _ => u64::from(GuestError::Unimplemented.as_raw()),
    }
}

/// `sceKeyboardClose(handle)`: an open handle answers 0 and frees its index for another open;
/// handle `-1` answers `0x80da0003`, as measured. Any other handle is unmeasured.
fn close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let handle = args[0];
    if handle as u32 == u32::MAX {
        return BAD_HANDLE;
    }
    let Some(index) = open_index(handle) else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if let Ok(mut open) = OPEN.lock() {
        open[index] = false;
    }
    0
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The keyboard's open state is the process's, so the tests that open it take turns.
    static TURN: Mutex<()> = Mutex::new(());

    fn call(name: &str, args: &[u64]) -> u64 {
        let (_, f) = super::implementations()
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is implemented"));
        let mut regs = [0_u64; GUEST_ARG_REGISTERS];
        regs[..args.len()].copy_from_slice(args);
        f(&regs)
    }

    /// The keyboard's life as `101-input-ext/keyboard-lifecycle` measured it, with the host's held
    /// keys in the record (D783).
    #[test]
    fn a_keyboard_opens_reads_the_host_keys_and_closes_as_measured() {
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let user = u64::from(orbistoun_systemservice::signed_in_user());
        assert_eq!(call("sceKeyboardInit", &[]), 0);
        assert_eq!(call("sceKeyboardOpen", &[0, 0, 0, 0]), 0x809b_0081);
        assert_eq!(
            call("sceKeyboardOpen", &[u64::from(u32::MAX), 0, 0, 0]),
            0x809b_0001
        );
        let handle = call("sceKeyboardOpen", &[user, 0, 0, 0]);
        assert!(handle as i32 > 0, "a positive handle");
        assert_eq!(call("sceKeyboardOpen", &[user, 0, 0, 0]), 0x80da_0004);

        super::set_held(&[0x04, 0x2c]);
        let mut state = [0xa5_u8; 0x80];
        let at = state.as_mut_ptr() as u64;
        assert_eq!(
            call("sceKeyboardReadState", &[u64::from(u32::MAX), at]),
            0x80da_0003
        );
        assert_eq!(call("sceKeyboardReadState", &[handle, at]), 0);
        assert_eq!(state[0x10], 1, "connected");
        assert_eq!(state[0x14..0x18], 2_u32.to_le_bytes(), "two keys down");
        super::set_held(&[]);
        assert_eq!(call("sceKeyboardReadState", &[handle, at]), 0);
        assert_eq!(
            state[0x14..0x18],
            1_u32.to_le_bytes(),
            "a length of 1 at rest, as both console reads wrote"
        );
        assert!(state[0x20..0x60].iter().all(|&b| b == 0), "no key held");
        super::set_held(&[0x04, 0x2c]);
        assert_eq!(call("sceKeyboardReadState", &[handle, at]), 0);
        assert_eq!(state[0x20..0x24], [0x04, 0, 0x2c, 0], "A and space");
        assert!(
            state[0x24..0x60].iter().all(|&b| b == 0),
            "the rest of 96 bytes zero"
        );
        assert!(
            state[0x60..].iter().all(|&b| b == 0xa5),
            "nothing past 96 bytes"
        );

        assert_eq!(
            call("sceKeyboardClose", &[u64::from(u32::MAX)]),
            0x80da_0003
        );
        assert_eq!(call("sceKeyboardClose", &[handle]), 0);
        super::set_held(&[]);
    }

    /// `sceKeyboardSetProcessPrivilege` answers -1 for 1 and 0; `sceKeyboardSetProcessFocus` -1 for 1
    /// and `0x80da0001` for 0 (`100-input/keyboard-privilege-focus`, `20261010-115300-eboot.log`
    /// 4227-4246).
    #[test]
    fn privilege_and_focus_answer_as_measured() {
        assert_eq!(call("sceKeyboardSetProcessPrivilege", &[1]), 0xffff_ffff);
        assert_eq!(call("sceKeyboardSetProcessPrivilege", &[0]), 0xffff_ffff);
        assert_eq!(call("sceKeyboardSetProcessFocus", &[1]), 0xffff_ffff);
        assert_eq!(call("sceKeyboardSetProcessFocus", &[0]), 0x80da_0001);
    }

    /// The same user opens index 0 and index 1 and gets two handles, as oops-sdk does on a console
    /// (Craft's hardware capture of 2026-10-06); only a repeat at an index already open is refused.
    #[test]
    fn a_user_opens_a_second_index_as_oops_sdk_does_on_a_console() {
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let user = u64::from(orbistoun_systemservice::signed_in_user());
        let first = call("sceKeyboardOpen", &[user, 0, 0, 0]);
        let second = call("sceKeyboardOpen", &[user, 0, 1, 0]);
        assert!(first as i32 > 0 && second as i32 > 0, "two handles");
        assert_ne!(first, second);
        assert_eq!(call("sceKeyboardOpen", &[user, 0, 1, 0]), 0x80da_0004);
        let mut state = [0_u8; 96];
        let at = state.as_mut_ptr() as u64;
        assert_eq!(call("sceKeyboardReadState", &[second, at]), 0);
        assert_eq!(call("sceKeyboardClose", &[second]), 0);
        assert_eq!(call("sceKeyboardClose", &[first]), 0);
    }
}
