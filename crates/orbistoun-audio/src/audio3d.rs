//! `libSceAudio3d` - positional audio.
//!
//! Declared and not implemented. The names come from real import tables (D504); every
//! arity is `6`, the trampoline's full capture, not a claim about the argument count.
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAudio3d" {
        "sceAudio3dInitialize" => 6,
        "sceAudio3dObjectReserve" => 6,
        "sceAudio3dObjectSetAttributes" => 6,
        "sceAudio3dPortClose" => 6,
        "sceAudio3dPortFlush" => 6,
        "sceAudio3dPortOpen" => 6,
        "sceAudio3dTerminate" => 6,
    }
}
