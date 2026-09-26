//! `libSceAudioOut2` - the second-generation audio output interface, beside `libSceAudioOut`.
//!
//! Declared and not implemented. The names come from real import tables (D504); every
//! arity is `6`, the trampoline's full capture, not a claim about the argument count.
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

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
        "sceAudioOut2UserDestroy" => 6,
    }
}
