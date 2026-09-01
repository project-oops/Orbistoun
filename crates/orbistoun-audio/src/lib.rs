//! Audio output HLE - libSceAudioOut.
//!
//! Audio is the subsystem most often stubbed to silence and left there, which is
//! a mistake worth naming: guests frequently block on audio-buffer completion, so
//! a stub that never signals a drained buffer hangs the title with no audio
//! symptom to point at it.
//!
//! # Status
//!
//! Declarations only. Arities are provisional.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

/// The high half libSceAudioOut numbers its errors from, rather than the kernel's `0x8002_0000`.
///
/// **Measured.** obSCEne's `090-audio/close-rejects-bad-handle` answered `0x80260003` on hardware -
/// the audio subsystem's own `NO_SUCH`. A guest that checks for `SCE_AUDIO_OUT_ERROR_*` never matches
/// the kernel base or a `0x7fff…` placeholder, so the base belongs here where the shim knows it, the
/// same way [`orbistoun_input`]'s pad shim carries its own (D438).
const AUDIO_ERROR_BASE: u32 = 0x8026_0000;

guest_module! {
    "libSceAudioOut" {
        "sceAudioOutInit" => 0,
        "sceAudioOutOpen" => 6,
        "sceAudioOutClose" => 1,
        "sceAudioOutOutput" => 2,
        "sceAudioOutSetVolume" => 3,
    }
}

/// `sceAudioOutInit()` - initialise the audio-output library.
///
/// There is no audio backend yet, and the port calls stay unimplemented - writing samples
/// nowhere and reporting success would be the D171 shape. But *initialising* is the precondition
/// a title checks before it opens a port, and left unimplemented this fell to the placeholder,
/// which answers a non-zero code that `090-audio/initialise` reads as a failed init. Answering
/// success lets a title reach the port calls, which then fail honestly. The library is
/// idempotent-init on hardware, so repeating it is also success.
fn audio_out_init(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `sceAudioOutClose(handle)` - close an audio-output port.
///
/// Closing does not move samples, so it is not one of the calls held back for the missing backend -
/// but there is no `Open` either, so no port handle is ever valid here, and every close is a close of
/// a handle that names nothing. Answered as the audio subsystem's `NO_SUCH` (`0x80260003`), the code
/// obSCEne measured for exactly this, rather than the placeholder a guest testing for
/// `SCE_AUDIO_OUT_ERROR_*` would not recognise. Honest either way: it rejects rather than pretending a
/// port closed.
fn audio_out_close(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(GuestError::vendor_in(AUDIO_ERROR_BASE, orbistoun_core::errno::NO_SUCH).as_raw())
}

/// The audio implementations this crate provides. `Init` and `Close`, deliberately no more: the calls
/// that would move samples have no backend and stay unimplemented rather than pretend (principle 3).
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAudioOutInit", audio_out_init),
        ("sceAudioOutClose", audio_out_close),
    ]
}
