//! `libSceNpUniversalDataSystem` - the platform's event and statistics reporting.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceNpUniversalDataSystemInitialize` and
//! `sceNpUniversalDataSystemCreateContext` are implemented, as measured.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceNpUniversalDataSystem" {
        // (param)
        // (&ctx, user, 0, 0)
        "sceNpUniversalDataSystemCreateContext" => 4,
        "sceNpUniversalDataSystemInitialize" => 1,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpUniversalDataSystemInitialize", initialize),
        ("sceNpUniversalDataSystemCreateContext", create_context),
    ]
}

/// `sceNpUniversalDataSystemInitialize(param)`: with `param` u64 0x10 (its size), u64 0x40000 (a
/// memory size), a pointer and u64 0x10, as PPSA02664's plugin passes it, answers 0 and leaves the
/// block as it was (REQ-np02, two sweeps: `20261011-083754-eboot.obs.log` 2933-2960 and
/// `20261011-1791715554-eboot.obs.log` 2987-3014, after `sceSysmoduleLoadModule(0x0105)`). A null
/// block is unmeasured and answers the placeholder.
fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    0
}

/// The context a fresh process's first `sceNpUniversalDataSystemCreateContext` gave (REQ-np03,
/// `uds-create-context-call1`); each later one is the next.
static NEXT_CONTEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0004_00d3);

/// `sceNpUniversalDataSystemCreateContext(&ctx, user, 0, 0)`: writes the next context number to
/// the low 32 bits of `ctx`, leaving the high half as it was, and answers 0 - `0x400d3` then
/// `0x400d4` on the console (REQ-np03, `000-boot/np-appcontent-startup`, `report-1791758178.txt`
/// 223-266). A null `ctx` is unmeasured and answers the placeholder.
fn create_context(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let number = NEXT_CONTEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the guest's context slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u32(args[0], number) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}
