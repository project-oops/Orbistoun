//! `libSceContentSearch` - searching the console's own media (captured videos and screenshots).
//!
//! Content search is local: an offline console answers it from its own storage. orbistoun keeps
//! no captures, so a search starts, finds nothing and ends. PPSA04263 starts the library at boot
//! (after `sceSysmoduleLoadModule(0xc7)`) and its video-file enumerator searches for clips; on a
//! search answering 0 hits it goes straight to its finish path (image+0xd0e9f8), so the metadata
//! calls - reached only with hits - are left declared by the title's import table alone.
//!
//! Assumed, not measured: the answers are an offline console's with no captures; obSCEne has not
//! probed the library (census REQ-20261012T1818Z-3426 asked, without rows).

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceContentSearch" {
        // (param *): `{ u64 memory size }` - PPSA04263 lends 0x400000.
        "sceContentSearchInit" => 1,
        "sceContentSearchTerm" => 0,
        // Nine arguments; the seventh, the first on the stack, takes the hit count.
        "sceContentSearchSearchContent" => 6,
    }
}

/// Whether the library has been started and not ended.
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `sceContentSearchInit(param)`: starts the library and answers 0. A null parameter answers the
/// placeholder.
fn init(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if args[0] == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    STARTED.store(true, std::sync::atomic::Ordering::Release);
    0
}

/// `sceContentSearchTerm()`: ends the library and answers 0; before `sceContentSearchInit` it
/// answers the placeholder.
fn term(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if STARTED.swap(false, std::sync::atomic::Ordering::AcqRel) {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceContentSearchSearchContent(...)`: finds no content and answers 0, writing a hit count of 0
/// through the seventh argument, the first on the stack. Before `sceContentSearchInit`, or with no
/// count to write, it answers the placeholder.
fn search_content(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let area = orbistoun_thunk::stack_arguments();
    // Zero outside a guest call, which reads as no count.
    let count_at = if area == 0 {
        0
    } else {
        // SAFETY: the guest's own stack, the word above the return address of the call running
        // now; read through the checked accessor.
        unsafe { guest::read_u64(area) }.unwrap_or(0)
    };
    search_with(count_at)
}

/// [`search_content`] with its count destination read.
fn search_with(count_at: u64) -> u64 {
    if !STARTED.load(std::sync::atomic::Ordering::Acquire) || count_at == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    // SAFETY: the guest's hit count, one 32-bit word by the call's contract; written through the
    // checked accessor.
    if unsafe { guest::write_u32(count_at, 0) } {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Implementations this module provides, by symbol name.
#[must_use]
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceContentSearchInit", init),
        ("sceContentSearchTerm", term),
        ("sceContentSearchSearchContent", search_content),
    ]
}

#[cfg(test)]
mod tests {
    /// A search between start and end finds nothing; before the start, after the end, or with no
    /// count to write, it is refused. One test: the started flag is the process's.
    #[test]
    fn a_search_finds_nothing_between_start_and_end() {
        let mut count = 7_u32;
        let at = std::ptr::from_mut(&mut count) as u64;
        assert_ne!(super::search_with(at), 0, "not started");
        let param = 0x40_0000_u64;
        let param_at = std::ptr::from_ref(&param) as u64;
        assert_eq!(super::init(&[param_at, 0, 0, 0, 0, 0]), 0);
        assert_eq!(super::search_with(at), 0);
        assert_eq!(count, 0);
        assert_ne!(super::search_with(0), 0, "no count");
        assert_eq!(super::term(&[0; 6]), 0);
        assert_ne!(super::term(&[0; 6]), 0, "ended already");
        assert_ne!(super::init(&[0; 6]), 0, "no parameter");
    }
}
