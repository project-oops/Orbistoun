//! `libSceNpManager` - account sign-in and presence.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceNpGetAccountCountryA`,
//! `sceNpGetAccountIdA` and `sceNpCheckCallback` are implemented, as measured.

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
        "sceNpGetState" => 6,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpCheckCallback", check_callback),
        ("sceNpGetAccountCountryA", get_account_country_a),
        ("sceNpGetAccountIdA", get_account_id_a),
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
