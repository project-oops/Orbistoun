//! `libSceAudioIn` - audio capture.
//!
//! Declared and not implemented. The names come from real import tables (D504); every
//! arity is `6`, the trampoline's full capture, not a claim about the argument count.
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAudioIn" {
        "sceAudioInClose" => 6,
        "sceAudioInInput" => 6,
        "sceAudioInOpen" => 6,
    }
}
