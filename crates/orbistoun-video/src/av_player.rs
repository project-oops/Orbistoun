//! `libSceAvPlayer` - the bundled media player, which decodes and presents a stream on the title's behalf.
//!
//! **22 names, declared and not implemented.** They come from PPSA02664's own import table (22).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAvPlayer" {
        "sceAvPlayerAddSourceEx" => 6,
        "sceAvPlayerChangeStream" => 6,
        "sceAvPlayerClose" => 6,
        "sceAvPlayerCurrentTime" => 6,
        "sceAvPlayerEnableStream" => 6,
        "sceAvPlayerGetAudioData" => 6,
        "sceAvPlayerGetStreamInfo" => 6,
        "sceAvPlayerGetStreamInfoEx" => 6,
        "sceAvPlayerGetVideoDataEx" => 6,
        "sceAvPlayerInitEx" => 6,
        "sceAvPlayerIsActive" => 6,
        "sceAvPlayerJumpToTime" => 6,
        "sceAvPlayerPause" => 6,
        "sceAvPlayerPostInit" => 6,
        "sceAvPlayerResume" => 6,
        "sceAvPlayerSetAvSyncMode" => 6,
        "sceAvPlayerSetLogCallback" => 6,
        "sceAvPlayerSetLooping" => 6,
        "sceAvPlayerSetTrickSpeed" => 6,
        "sceAvPlayerStart" => 6,
        "sceAvPlayerStop" => 6,
        "sceAvPlayerStreamCount" => 6,
    }
}
