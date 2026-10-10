//! `libSceAudioOut2` - the second-generation audio output interface, beside `libSceAudioOut`.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceAudioOut2UserCreate` and
//! `sceAudioOut2UserDestroy` are implemented, as measured.

use std::sync::atomic::{AtomicU64, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceAudioOut2" {
        "sceAudioOut2ContextAdvance" => 6,
        "sceAudioOut2ContextCreate" => 6,
        "sceAudioOut2ContextDestroy" => 6,
        "sceAudioOut2ContextGetQueueLevel" => 6,
        "sceAudioOut2ContextPush" => 6,
        "sceAudioOut2ContextQueryMemory" => 6,
        "sceAudioOut2ContextResetParam" => 6,
        "sceAudioOut2Initialize" => 6,
        "sceAudioOut2PortCreate" => 6,
        "sceAudioOut2PortDestroy" => 6,
        "sceAudioOut2PortSetAttributes" => 6,
        // (user, out)
        "sceAudioOut2UserCreate" => 2,
        // (user)
        "sceAudioOut2UserDestroy" => 1,
    }
}

/// The next handle `sceAudioOut2UserCreate` gives: small and non-zero, not an address (D151). The
/// console's were heap addresses (`0x80067fbf0`, `0x80067fc10`), so no value of them is a contract.
static NEXT_USER: AtomicU64 = AtomicU64::new(1);

/// `sceAudioOut2UserCreate(user, out)`: for the signed-in user, writes an 8-byte handle to `out`
/// and answers 0, with the module loaded or not (REQ-ao01, `20261010-100600-eboot.obs.log`, the
/// 0x40-byte `out` changed in exactly its first 8 bytes). Any other user, and a null `out`, are
/// unmeasured and answer the placeholder.
pub(crate) fn user_create(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (user, out) = (args[0], args[1]);
    if user as u32 != orbistoun_systemservice::signed_in_user() || out == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let handle = NEXT_USER.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's 8-byte out-parameter, by the call's contract.
    if unsafe { guest::write_u64(out, handle) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// What `sceAudioOut2UserDestroy` answered for a user `sceAudioOut2UserCreate` made (`0x80268001`)
/// and for 0 alike (REQ-cn10 arm 2, `20261009-151440-eboot.obs.log`), and again for the handle a
/// successful create wrote (REQ-ao01, `20261010-100600-eboot.obs.log`).
const USER_DESTROY_REFUSED: u64 = 0x8026_8010;

/// `sceAudioOut2UserDestroy(user)`: answers `0x80268010`, as the console answered whatever it was
/// handed.
pub(crate) fn user_destroy(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    USER_DESTROY_REFUSED
}
