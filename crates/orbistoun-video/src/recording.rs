//! `libSceVideoRecording` - game-capture recording.
//!
//! The declared names are the ones modules in the recorded corpus call; a name from a candidate
//! list without an import or a resolution is not declared. None is implemented, so the module is
//! listed in `SERVES_NOTHING`.
//!
//! Every arity is `6`, the trampoline's full capture, not a claim about how many arguments a
//! function takes.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceVideoRecording" {
        "sceVideoRecordingClose" => 6,
        "sceVideoRecordingGetStatus" => 6,
        "sceVideoRecordingQueryMemSize" => 6,
        "sceVideoRecordingStop" => 6,
    }
}
