//! `libSceLibcInternalExt`: the heap-tracing hook a title's own libc asks for while it sets up its
//! heap (D770).
//!
//! A title's `libc.prx` passes a 32-byte block it has filled from a template, then reads three
//! fields back (PPSA28061 `libc.prx` `_malloc_init`, `+0x1aec1`..`+0x1af9a`): a word at `+0x0c`
//! that switches tracing on when non-zero, and two pointers at `+0x10` and `+0x18`, which it keeps
//! and writes through - zero to the first straight away, and its trace state to the second when
//! tracing is on.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceLibcInternalExt" {
        // A pointer to the caller's 32-byte block.
        "sceLibcHeapGetTraceInfo" => 1,
    }
}

/// Where in the block the switch that turns tracing on is.
const TRACING: u64 = 0x0c;
/// Where in the block the first word the caller writes through is.
const FIRST_WORD: u64 = 0x10;
/// Where in the block the second word the caller writes through is.
const SECOND_WORD: u64 = 0x18;

/// The two words the caller keeps and writes through: orbistoun's own, zeroed, never freed.
fn trace_words() -> u64 {
    static WORDS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *WORDS.get_or_init(|| orbistoun_mem::blocks::block(4))
}

/// `sceLibcHeapGetTraceInfo(info)`: tracing off, and two words the caller may write (D770).
///
/// A retail process runs without the heap tracer, so the switch is cleared, and the two pointers
/// name zeroed words orbistoun owns rather than a tracer's shared state. The rest of the block is
/// left as the caller filled it. Answers `0`; the caller does not test the answer.
fn heap_get_trace_info(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let info = args[0];
    let words = trace_words();
    // SAFETY: an address the guest passed for this call, valid by its contract: a 32-byte block,
    // and every field written lies inside it.
    let written = unsafe { guest::write_u32(info + TRACING, 0) }
        // SAFETY: as above.
        && unsafe { guest::write_u64(info + FIRST_WORD, words) }
        // SAFETY: as above.
        && unsafe { guest::write_u64(info + SECOND_WORD, words + 0x10) };
    if written {
        0
    } else {
        u64::from(orbistoun_core::GuestError::InvalidArgument.as_raw())
    }
}

/// Everything here, by symbol name.
pub(crate) fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[("sceLibcHeapGetTraceInfo", heap_get_trace_info)]
}

#[cfg(test)]
mod tests {
    /// Tracing is reported off, the two words are distinct writable memory, and the caller's other
    /// fields are left alone (D770).
    #[test]
    fn the_heap_is_reported_untraced_with_two_words_to_write() {
        let info = orbistoun_mem::blocks::block(4);
        // SAFETY: the block just handed out is writable.
        assert!(unsafe { super::guest::write_u64(info, 0x0000_0005_0000_0004) });
        // SAFETY: as above.
        assert!(unsafe { super::guest::write_u64(info + 8, 0x0000_0005_0000_0005) });
        assert_eq!(super::heap_get_trace_info(&[info, 0, 0, 0, 0, 0]), 0);
        // SAFETY: inside the block.
        let read = |at: u64| unsafe { super::guest::read_u64(info + at) };
        assert_eq!(
            read(0),
            Some(0x0000_0005_0000_0004),
            "left as the caller wrote it"
        );
        assert_eq!(
            read(8),
            Some(0x0000_0000_0000_0005),
            "the switch is cleared"
        );
        let (first, second) = (read(0x10).expect("first"), read(0x18).expect("second"));
        assert_ne!(first, 0);
        assert_ne!(first, second);
        // SAFETY: the words this call handed out.
        assert!(unsafe { super::guest::write_u64(first, 0) });
        // SAFETY: as above.
        assert!(unsafe { super::guest::write_u64(second, 7) });
    }
}
