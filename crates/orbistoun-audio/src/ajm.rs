//! `libSceAjm` - the platform's audio codec job manager: decode and encode work submitted as jobs.
//!
//! Declared and not implemented. The names come from real import tables (D504); every
//! arity is `6`, the trampoline's full capture, not a claim about the argument count.
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
