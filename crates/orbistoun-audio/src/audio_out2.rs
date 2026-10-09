//! `libSceAudioOut2` - the second-generation audio output interface, beside `libSceAudioOut`.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. Only `sceAudioOut2UserDestroy` is
//! implemented, as measured.

use orbistoun_hle::guest_module;

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
        "sceAudioOut2UserCreate" => 6,
        // (user)
        "sceAudioOut2UserDestroy" => 1,
    }
}

/// What `sceAudioOut2UserDestroy` answered for a user `sceAudioOut2UserCreate` made (`0x80268001`)
/// and for 0 alike (REQ-cn10 arm 2, `20261009-151440-eboot.obs.log`).
const USER_DESTROY_REFUSED: u64 = 0x8026_8010;

/// `sceAudioOut2UserDestroy(user)`: answers `0x80268010`, as the console answered whatever it was
/// handed.
pub(crate) fn user_destroy(_args: &[u64; orbistoun_core::GUEST_ARG_REGISTERS]) -> u64 {
    USER_DESTROY_REFUSED
}
