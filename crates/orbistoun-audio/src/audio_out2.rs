//! `libSceAudioOut2` - the second-generation audio output interface, beside the `libSceAudioOut` this crate already declares.
//!
//! **13 names, declared and not implemented.** They come from PPSA02664's own import table (13).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
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
