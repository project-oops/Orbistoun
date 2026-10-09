//! `libSceAjm` - the platform's audio codec job manager: decode and encode work submitted as jobs.
//!
//! The names come from real import tables (D504); every arity is `6`, the trampoline's full
//! capture, not a claim about the argument count. Only `sceAjmInitialize` is implemented, as
//! measured; the codec jobs themselves are not.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_hle::guest_module;
use std::sync::atomic::{AtomicU32, Ordering};

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
        // (0, &ctx), as measured.
        "sceAjmInitialize" => 2,
        "sceAjmInstanceCreate" => 6,
        "sceAjmInstanceDestroy" => 6,
        "sceAjmModuleRegister" => 6,
        "sceAjmModuleUnregister" => 6,
    }
}

/// `SCE_AJM_ERROR_INVALID_PARAMETER`-family answer the census measured for a non-zero first
/// argument and for a null slot (`0x80930005`).
const INVALID: u64 = 0x8093_0005;

/// The next context identifier: `0x15` first, as REQ-cn10 arm 1 measured a fresh process's first
/// context, then one more each time.
static NEXT_CONTEXT: AtomicU32 = AtomicU32::new(0x15);

/// `sceAjmInitialize(0, &ctx)`: writes a 32-bit context identifier and answers 0, the four bytes
/// after it left as they were (`call1-ctx` `15000000ffffffff`, `call2-ctx` `16000000...`,
/// `20261009-151440-eboot.obs.log`). A non-zero first argument or a null slot answers
/// `0x80930005`, as the census's pattern and zero arguments were answered.
pub(crate) fn ajm_initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (reserved, out) = (args[0], args[1]);
    if reserved != 0 || out == 0 {
        return INVALID;
    }
    let context = NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed);
    // SAFETY: the guest's out-parameter, a four-byte context identifier by the call's contract.
    if unsafe { orbistoun_mem::guest::write_u32(out, context) } {
        0
    } else {
        INVALID
    }
}
