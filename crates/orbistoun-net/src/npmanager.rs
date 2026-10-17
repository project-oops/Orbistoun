//! `libSceNpManager` - account sign-in and presence.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceNpGetAccountCountryA`,
//! `sceNpGetAccountIdA`, `sceNpGetState`, `sceNpSetNpTitleId` and `sceNpCheckCallback` are
//! implemented, as measured.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceNpManager" {
        "sceNpCheckCallback" => 0,
        // (user, out)
        "sceNpGetAccountCountryA" => 2,
        // (user, out)
        "sceNpGetAccountIdA" => 2,
        "sceNpGetNpReachabilityState" => 6,
        // (user, out)
        "sceNpGetState" => 2,
        // (function, userdata)
        "sceNpRegisterStateCallbackA" => 2,
        // (id)
        "sceNpUnregisterStateCallbackA" => 1,
        // (&id, &secret)
        "sceNpSetNpTitleId" => 2,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpCheckCallback", check_callback),
        ("sceNpGetAccountCountryA", get_account_country_a),
        ("sceNpGetAccountIdA", get_account_id_a),
        ("sceNpGetState", get_state),
        ("sceNpRegisterStateCallbackA", register_state_callback_a),
        ("sceNpUnregisterStateCallbackA", unregister_state_callback_a),
        ("sceNpSetNpTitleId", set_np_title_id),
    ]
}

/// `sceNpCheckCallback()`: runs the state callbacks a title registered and answers 0, on the
/// caller's thread. Measured with none registered, ten times on the main thread and once on a
/// second (`070-user/np-check-callback`, `20261010-115300-eboot.log` 3540-3558).
///
/// The console is offline and its user signed in to no network account, so the one state there is
/// to report is signed out, the state [`get_state`] answers; each registration is owed that one
/// delivery - `(user, SIGNED_OUT, userdata)` - and later pumps deliver nothing, since nothing
/// changes. Assumed from the shipped module's slot handling as the reference emulators read it; the
/// hardware census REQ-gt06 replaces it if it disagrees.
fn check_callback(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let owed = state_callbacks()
        .lock()
        .map(|mut table| table.owed())
        .unwrap_or_default();
    let user = u64::from(orbistoun_systemservice::signed_in_user());
    for (function, userdata) in owed {
        // SAFETY: `function` is the guest's own state callback, handed over by
        // `sceNpRegisterStateCallbackA`; it is called as the library would, with the user, the state
        // and the guest's own userdata.
        let _ = unsafe {
            orbistoun_kernel::thread::call_guest(function, [user, u64::from(SIGNED_OUT), userdata])
        };
    }
    0
}

/// `SCE_NP_ERROR_INVALID_ARGUMENT`: a null callback.
const NP_INVALID_ARGUMENT: u64 = 0x8055_0003;
/// `SCE_NP_ERROR_CALLBACK_ALREADY_REGISTERED`: the same function registered twice.
const NP_CALLBACK_ALREADY_REGISTERED: u64 = 0x8055_0008;
/// `SCE_NP_ERROR_CALLBACK_NOT_REGISTERED`: an id whose slot is empty.
const NP_CALLBACK_NOT_REGISTERED: u64 = 0x8055_0009;
/// An id outside the table's slots.
const NP_INVALID_CALLBACK_ID: u64 = 0x8055_0015;
/// `SCE_NP_ERROR_CALLBACK_MAX`: every slot taken.
const NP_CALLBACK_MAX: u64 = 0x8055_001d;

/// How many state callbacks the library keeps.
const STATE_CALLBACK_SLOTS: usize = 8;

/// The registered state callbacks: each slot's function, userdata and whether its one delivery is
/// still owed. Pure bookkeeping, so it is tested without a guest; [`check_callback`] calls them.
#[derive(Debug, Default)]
struct StateCallbacks {
    slots: [Option<(u64, u64, bool)>; STATE_CALLBACK_SLOTS],
}

impl StateCallbacks {
    /// Registers `function` with `userdata` in the lowest free slot, answering the slot plus one, or
    /// the library's refusal: a null function, one already in a slot (checked before fullness), or
    /// a full table.
    fn register(&mut self, function: u64, userdata: u64) -> u64 {
        if function == 0 {
            return NP_INVALID_ARGUMENT;
        }
        if self
            .slots
            .iter()
            .flatten()
            .any(|&(held, ..)| held == function)
        {
            return NP_CALLBACK_ALREADY_REGISTERED;
        }
        let Some(free) = self.slots.iter().position(Option::is_none) else {
            return NP_CALLBACK_MAX;
        };
        self.slots[free] = Some((function, userdata, true));
        free as u64 + 1
    }

    /// Frees the slot `id` names: 0, or the library's refusal for an id outside the slots or one
    /// whose slot is empty.
    fn unregister(&mut self, id: u64) -> u64 {
        let Some(slot) = id
            .checked_sub(1)
            .and_then(|slot| usize::try_from(slot).ok())
            .and_then(|slot| self.slots.get_mut(slot))
        else {
            return NP_INVALID_CALLBACK_ID;
        };
        if slot.take().is_none() {
            return NP_CALLBACK_NOT_REGISTERED;
        }
        0
    }

    /// The callbacks still owed their delivery, marked delivered: `(function, userdata)`.
    fn owed(&mut self) -> Vec<(u64, u64)> {
        self.slots
            .iter_mut()
            .flatten()
            .filter(|(_, _, owed)| *owed)
            .map(|(function, userdata, owed)| {
                *owed = false;
                (*function, *userdata)
            })
            .collect()
    }
}

/// The process's state callbacks.
fn state_callbacks() -> &'static std::sync::Mutex<StateCallbacks> {
    static TABLE: std::sync::OnceLock<std::sync::Mutex<StateCallbacks>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(Default::default)
}

/// `sceNpRegisterStateCallbackA(function, userdata)`: the callback's id, 1 to 8, or a refusal.
fn register_state_callback_a(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    state_callbacks()
        .lock()
        .map_or(NP_CALLBACK_MAX, |mut table| {
            table.register(args[0], args[1])
        })
}

/// `sceNpUnregisterStateCallbackA(id)`: 0, or a refusal.
fn unregister_state_callback_a(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    state_callbacks()
        .lock()
        .map_or(NP_CALLBACK_NOT_REGISTERED, |mut table| {
            table.unregister(args[0] & 0xffff_ffff)
        })
}

/// What `sceNpGetAccountCountryA` answered on a console signed in to no account (REQ-cn10 arm 7,
/// `20261009-151440-eboot.obs.log`).
const NOT_SIGNED_IN: u64 = 0x8055_0006;

/// `sceNpGetAccountCountryA(user, out)`: no account is signed in, so the console's refusal, with
/// `out` left as it was.
fn get_account_country_a(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    NOT_SIGNED_IN
}

/// `sceNpGetAccountIdA(user, out)`: no account is signed in, so the console's refusal, with the
/// eight bytes of `out` zeroed, alike twice (REQ-np03, `000-boot/np-appcontent-startup`,
/// `report-1791758178.txt` 223-266). A null `out` is unmeasured and answers the placeholder.
fn get_account_id_a(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's 8-byte account id, by the call's contract; written through the checked
    // accessor.
    if args[1] != 0 && unsafe { orbistoun_mem::guest::write_u64(args[1], 0) } {
        NOT_SIGNED_IN
    } else {
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    }
}

/// The state `sceNpGetState` wrote on a console signed in to no account: 1, signed out (REQ-np05,
/// `000-boot/np05-routes-calls`, `report-20261014-batch4.log` 2883-2884).
const SIGNED_OUT: u32 = 1;

/// `sceNpGetState(user, out)`: no account is signed in, so 0 with the signed-out state written as
/// four bytes, as measured. A null `out` is unmeasured and answers the placeholder.
fn get_state(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's 4-byte state, by the call's contract; written through the checked
    // accessor.
    if args[1] != 0 && unsafe { orbistoun_mem::guest::write_u32(args[1], SIGNED_OUT) } {
        0
    } else {
        u64::from(orbistoun_core::GuestError::Unimplemented.as_raw())
    }
}

/// The NP title id block `sceNpSetNpTitleId` is handed: twelve characters and padding.
const TITLE_ID_BYTES: usize = 0x10;

/// The secret block handed beside it.
const TITLE_SECRET_BYTES: usize = 0x80;

/// What `sceNpSetNpTitleId` answered for zeroed blocks, on a console signed in to no account
/// (REQ-np05, `report-20261014-batch4.log` 2885).
const ZEROED_TITLE_ID: u64 = 0x8055_001e;

/// `sceNpSetNpTitleId(&id, &secret)`: 0 for a title id and a non-zero secret - PPSA04263's shape,
/// alike twice (REQ-gt01, `000-boot/gt01-npmanager`, `20261015-164000-eboot.log` 4841-4846) - and
/// `0x8055001e` when both blocks are zero (REQ-np05). Any other shape - one block zero and not the
/// other, or a null pointer - is unmeasured and answers the placeholder.
fn set_np_title_id(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (mut id, mut secret) = ([0_u8; TITLE_ID_BYTES], [0_u8; TITLE_SECRET_BYTES]);
    // SAFETY: the guest's title id block, by the call's contract; read through the checked
    // accessor.
    let read_id = args[0] != 0 && unsafe { orbistoun_mem::guest::read_into(args[0], &mut id) };
    // SAFETY: the guest's secret block, likewise.
    let read_secret =
        args[1] != 0 && unsafe { orbistoun_mem::guest::read_into(args[1], &mut secret) };
    if !(read_id && read_secret) {
        return u64::from(orbistoun_core::GuestError::Unimplemented.as_raw());
    }
    let zero = |block: &[u8]| block.iter().all(|&b| b == 0);
    match (zero(&id), zero(&secret)) {
        (false, false) => 0,
        (true, true) => ZEROED_TITLE_ID,
        _ => u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()),
    }
}

#[cfg(test)]
mod tests {
    use super::StateCallbacks;

    /// A registration takes the lowest free slot and answers it plus one; a null function, a
    /// function already registered and a full table are refused with their codes; each registration
    /// is owed one delivery, which a pump takes; an unregistered id frees its slot.
    #[test]
    fn state_callbacks_register_deliver_once_and_unregister() {
        let mut table = StateCallbacks::default();
        assert_eq!(table.register(0, 7), super::NP_INVALID_ARGUMENT);
        assert_eq!(table.register(0x1000, 7), 1);
        assert_eq!(
            table.register(0x1000, 8),
            super::NP_CALLBACK_ALREADY_REGISTERED
        );
        assert_eq!(table.register(0x2000, 9), 2);
        assert_eq!(table.owed(), [(0x1000, 7), (0x2000, 9)]);
        assert!(table.owed().is_empty(), "each delivered once");
        assert_eq!(table.unregister(1), 0);
        assert_eq!(table.unregister(1), super::NP_CALLBACK_NOT_REGISTERED);
        assert_eq!(table.unregister(0), super::NP_INVALID_CALLBACK_ID);
        assert_eq!(table.register(0x3000, 1), 1, "the freed slot is the lowest");
        for function in 0x4000..0x4006 {
            assert!(table.register(function, 0) < 9);
        }
        assert_eq!(table.register(0x5000, 0), super::NP_CALLBACK_MAX);
    }
}
