//! `libSceKeyboard` - keyboard input, read from the host's keyboard (D783).
//!
//! `sceKeyboardInit`, `sceKeyboardOpen`, `sceKeyboardReadState` and `sceKeyboardClose` answer as
//! obSCEne measured them (`100-input/keyboard-and-media-gates`, `101-input-ext/keyboard-lifecycle`
//! and the census, `20261009-151440-eboot.obs.log`). The keys a read reports are the host's, set
//! through [`set_held`] by whichever shell owns a keyboard; a run with none reports a connected
//! keyboard with nothing held. `sceKeyboardSetProcessPrivilege` and `sceKeyboardSetProcessFocus`
//! are unmeasured and declared only.

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
    ]
}

/// What an open for a user that is not signed in answered: user `-1`, and a buffer's address.
const NOT_A_USER: u64 = 0x809b_0001;
/// What an open with every argument zero answered.
const ZERO_ARGUMENTS: u64 = 0x809b_0081;
/// What a second open for a user who already has the keyboard open answered.
const ALREADY_OPEN: u64 = 0x80da_0004;
/// What a read or a close of handle `-1` answered.
const BAD_HANDLE: u64 = 0x80da_0003;

/// The handle an open hands out: small and positive, not an address (D151). The console's were
/// large and varied between runs (`0x16b0700`, `0x1720700`), so no value of them is a contract.
const HANDLE: u32 = 1;

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

/// Which user, if any, has the keyboard open.
static OPEN_FOR: Mutex<Option<u32>> = Mutex::new(None);

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

/// `sceKeyboardOpen(user, type, index, param)`: the signed-in user gets a handle; a second open for
/// them `0x80da0004`; every argument zero `0x809b0081`; any other user `0x809b0001`, as measured.
fn open(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args.iter().take(4).all(|&a| a == 0) {
        return ZERO_ARGUMENTS;
    }
    let user = args[0] as u32;
    if user != orbistoun_systemservice::signed_in_user() {
        return NOT_A_USER;
    }
    let Ok(mut open_for) = OPEN_FOR.lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    if open_for.is_some() {
        return ALREADY_OPEN;
    }
    *open_for = Some(user);
    u64::from(HANDLE)
}

/// Whether `handle` is the open keyboard's.
fn is_open(handle: u64) -> bool {
    handle as u32 == HANDLE && OPEN_FOR.lock().is_ok_and(|o| o.is_some())
}

/// The 96-byte record for the keys held now: present, how many keys are down, and which.
fn record(held: &[u16]) -> [u8; RECORD_BYTES] {
    let mut out = [0_u8; RECORD_BYTES];
    out[CONNECTED_AT] = 1;
    let count = held.len().min(MOST_KEYS);
    out[COUNT_AT..COUNT_AT + 4].copy_from_slice(&(count as u32).to_le_bytes());
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
    if !is_open(handle) {
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

/// `sceKeyboardClose(handle)`: the open keyboard's handle answers 0 and frees it for another open;
/// handle `-1` answers `0x80da0003`, as measured. Any other handle is unmeasured.
fn close(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let handle = args[0];
    if handle as u32 == u32::MAX {
        return BAD_HANDLE;
    }
    if !is_open(handle) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if let Ok(mut open_for) = OPEN_FOR.lock() {
        *open_for = None;
    }
    0
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

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
}
