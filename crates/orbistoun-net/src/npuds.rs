//! `libSceNpUniversalDataSystem` - the platform's event and statistics reporting.
//!
//! The names come from real import tables (D504); an unmeasured arity is `6`, the trampoline's
//! full capture, not a claim about the argument count. `sceNpUniversalDataSystemInitialize` and
//! `sceNpUniversalDataSystemCreateContext` and `sceNpUniversalDataSystemCreateHandle` are
//! implemented, as measured.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceNpUniversalDataSystem" {
        // (param)
        // (&ctx, user, 0, 0)
        "sceNpUniversalDataSystemCreateContext" => 4,
        "sceNpUniversalDataSystemInitialize" => 1,
        // (&handle)
        "sceNpUniversalDataSystemCreateHandle" => 1,
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceNpUniversalDataSystemInitialize", initialize),
        ("sceNpUniversalDataSystemCreateContext", create_context),
        ("sceNpUniversalDataSystemCreateHandle", create_handle),
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

/// The handle a fresh process's first `sceNpUniversalDataSystemCreateHandle` gave (REQ-pu01,
/// `000-boot/pad-ime-uds`, `report-1791801495.txt` 417-421); each later one is the next.
static NEXT_HANDLE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0002_0176);

/// `sceNpUniversalDataSystemCreateHandle(&handle)`: writes the next handle number to the low 32
/// bits of `handle`, leaving the high half as it was, and answers 0 - `0x20176` then `0x20177` on
/// the console, after a context was made. A null `handle` is unmeasured and answers the
/// placeholder.
fn create_handle(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let number = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the guest's handle slot, by the call's contract; written through the checked
    // accessor.
    if unsafe { orbistoun_mem::guest::write_u32(args[0], number) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// Handles count up from `0x20176` in the low four bytes, the rest of the slot as it was
    /// (REQ-pu01).
    #[test]
    fn handles_count_up_from_the_measured_first() {
        let mut slot = [0xcc_u8; 8];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = slot.as_mut_ptr() as u64;
        assert_eq!(super::create_handle(&args), 0);
        assert_eq!(slot, [0x76, 0x01, 0x02, 0x00, 0xcc, 0xcc, 0xcc, 0xcc]);
        assert_eq!(super::create_handle(&args), 0);
        assert_eq!(slot[..4], [0x77, 0x01, 0x02, 0x00]);
    }
}
