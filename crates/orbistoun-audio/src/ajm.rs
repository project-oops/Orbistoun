//! `libSceAjm` - the platform's audio codec job manager - decode and encode work submitted as jobs.
//!
//! **14 names, declared and not implemented.** They come from PPSA02664's own import table (14).
//!
//! Names confirmed, arities not: every arity here is `6`, the trampoline's full
//! capture, which is not a claim about how many arguments these take. The reasoning is
//! `orbistoun-gpu`'s `agc` module in full (D504); the short form is that a wrong arity
//! only degrades a trace while a wrong name is a shim nothing can reach.
//!
//! Listed in `SERVES_NOTHING` because nothing here is implemented.

use orbistoun_hle::guest_module;

guest_module! {
    "libSceAjm" {
        "sceAjmBatchCancel" => 6,
        "sceAjmBatchErrorDump" => 6,
        "sceAjmBatchInitialize" => 6,
        "sceAjmBatchJobDecode" => 6,
        "sceAjmBatchJobInitialize" => 6,
        "sceAjmBatchJobSetGaplessDecode" => 6,
        "sceAjmBatchStart" => 6,
        "sceAjmBatchWait" => 6,
        "sceAjmFinalize" => 6,
        "sceAjmInitialize" => 6,
        "sceAjmInstanceCreate" => 6,
        "sceAjmInstanceDestroy" => 6,
        "sceAjmModuleRegister" => 6,
        "sceAjmModuleUnregister" => 6,
    }
}
