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
        // (ctx).
        "sceAjmFinalize" => 1,
        // (0, &ctx), as measured.
        "sceAjmInitialize" => 2,
        "sceAjmInstanceCreate" => 6,
        "sceAjmInstanceDestroy" => 6,
        // (ctx, codec, 0).
        "sceAjmModuleRegister" => 3,
        "sceAjmModuleUnregister" => 6,
    }
}

/// `SCE_AJM_ERROR_INVALID_PARAMETER`-family answer the census measured for a non-zero first
/// argument and for a null slot (`0x80930005`).
const INVALID: u64 = 0x8093_0005;

/// The next context identifier: `0x15` first, as REQ-cn10 arm 1 measured a fresh process's first
/// context, then one more each time.
static NEXT_CONTEXT: AtomicU32 = AtomicU32::new(FIRST_CONTEXT);

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

/// What a repeat `sceAjmModuleRegister` of a codec answered (REQ-aj01).
const ALREADY_REGISTERED: u64 = 0x8093_0009;

/// The codecs measured registering: 0 to 3 (REQ-aj01).
const MEASURED_CODECS: u32 = 4;

/// The first context identifier `sceAjmInitialize` issues.
const FIRST_CONTEXT: u32 = 0x15;

/// Which codecs each context has registered, and which contexts are finalised.
#[derive(Default)]
struct Registry {
    /// `(context, codec)` pairs registered.
    registered: std::collections::BTreeSet<(u32, u32)>,
    /// Contexts finalised.
    finalised: std::collections::BTreeSet<u32>,
}

impl Registry {
    /// Whether `ctx` was issued below `issued_below` and is not finalised.
    fn live(&self, ctx: u32, issued_below: u32) -> bool {
        (FIRST_CONTEXT..issued_below).contains(&ctx) && !self.finalised.contains(&ctx)
    }

    /// `sceAjmModuleRegister`'s answer, or `None` where nothing measured it: a context that is not
    /// live, or a codec past those measured.
    fn register(&mut self, ctx: u32, codec: u32, issued_below: u32) -> Option<u64> {
        if !self.live(ctx, issued_below) || codec >= MEASURED_CODECS {
            return None;
        }
        Some(if self.registered.insert((ctx, codec)) {
            0
        } else {
            ALREADY_REGISTERED
        })
    }

    /// `sceAjmFinalize`'s answer for a live context, which it finalises; `None` for any other.
    fn finalize(&mut self, ctx: u32, issued_below: u32) -> Option<u64> {
        if !self.live(ctx, issued_below) {
            return None;
        }
        self.registered.retain(|&(c, _)| c != ctx);
        self.finalised.insert(ctx);
        Some(0)
    }
}

/// The process's AJM registry.
static REGISTRY: std::sync::Mutex<Option<Registry>> = std::sync::Mutex::new(None);

/// Runs `f` against the registry with the next context identifier, answering the placeholder
/// where it answers `None`.
fn with_registry(f: impl FnOnce(&mut Registry, u32) -> Option<u64>) -> u64 {
    let issued_below = NEXT_CONTEXT.load(Ordering::Relaxed);
    REGISTRY
        .lock()
        .ok()
        .and_then(|mut held| f(held.get_or_insert_with(Registry::default), issued_below))
        .unwrap_or_else(|| u64::from(orbistoun_core::GuestError::Unimplemented.as_raw()))
}

/// `sceAjmModuleRegister(ctx, codec, 0)`: 0 the first time a live context registers one of codecs
/// 0 to 3, `0x80930009` after (REQ-aj01, `20261009-215800-eboot.log` 4282-4290).
pub(crate) fn ajm_module_register(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_registry(|r, below| r.register(args[0] as u32, args[1] as u32, below))
}

/// `sceAjmFinalize(ctx)`: 0 for a live context, which it ends (REQ-aj01, lines 4291-4297).
pub(crate) fn ajm_finalize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    with_registry(|r, below| r.finalize(args[0] as u32, below))
}

#[cfg(test)]
mod tests {
    use super::{ALREADY_REGISTERED, Registry};

    /// `sceAjmModuleRegister(ctx, codec, 0)` answers 0 for codecs 0 to 3 and `0x80930009` when the
    /// codec is already registered; `sceAjmFinalize(ctx)` answers 0 (REQ-aj01,
    /// `20261009-215800-eboot.log` 4282-4297).
    #[test]
    fn modules_register_once_per_context_as_measured() {
        let mut registry = Registry::default();
        for codec in 0..4 {
            assert_eq!(registry.register(0x15, codec, 0x17), Some(0));
            assert_eq!(
                registry.register(0x15, codec, 0x17),
                Some(ALREADY_REGISTERED)
            );
        }
        assert_eq!(
            registry.register(0x99, 0, 0x17),
            None,
            "a context never issued"
        );
        assert_eq!(
            registry.register(0x15, 9, 0x17),
            None,
            "an unmeasured codec"
        );
        assert_eq!(registry.finalize(0x15, 0x17), Some(0));
        assert_eq!(registry.finalize(0x16, 0x17), Some(0));
        assert_eq!(
            registry.register(0x15, 0, 0x17),
            None,
            "a finalised context"
        );
    }
}
