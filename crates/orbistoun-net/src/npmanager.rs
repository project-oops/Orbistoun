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
        ("sceNpSetNpTitleId", set_np_title_id),
    ]
}

/// `sceNpCheckCallback()`: runs the callbacks a title registered and answers 0. Measured with none
/// registered, ten times on the main thread and once on a second (`070-user/np-check-callback`,
/// `20261010-115300-eboot.log` 3540-3558); nothing here registers one, so there is none to run.
fn check_callback(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
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
