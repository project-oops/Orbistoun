//! `libSceVideoRecording` - game-capture recording.
//!
//! **Four names, declared and not implemented**, and the four are the ones modules in the
//! recorded corpus are seen calling.
//!
//! # Six more names were here and have been taken out
//!
//! They came from obSCEne's `106-encoder/rec-symbols` check, which I read as measuring that
//! those symbols *resolve* on hardware. It measures the opposite: the record is emitted in the
//! branch where the lookup returned null, and the `0` beside it is filler for a status field.
//! There is not one `vaddr` or `handle` measurement in the whole encoder group - **every symbol
//! it probed failed to resolve** - and obSCEne's own `related-libs` check separately records
//! this library as *absent* in that process.
//!
//! So those six had no provenance at all: not an import table, not a resolution. A name from a
//! candidate list is the one thing this project's declarations are not allowed to be (D506).
//!
//! Names confirmed, arities not: arity `6` is the trampoline's full capture and not a claim.
//! See `orbistoun-gpu`'s `agc` module for the argument (D504).
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceVideoRecording" {
        "sceVideoRecordingClose" => 6,
        "sceVideoRecordingGetStatus" => 6,
        "sceVideoRecordingQueryMemSize" => 6,
        "sceVideoRecordingStop" => 6,
    }
}
