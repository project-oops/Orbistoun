//! `libSceAvPlayer` - the bundled media player, which decodes and presents a stream for a title.
//!
//! The names are declared and none is implemented, so the module is listed in `SERVES_NOTHING`.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

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
