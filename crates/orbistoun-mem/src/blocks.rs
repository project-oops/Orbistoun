//! Memory handed to the guest as a handle.
//!
//! A guest reads fields through what it is given, so a handle is the address of a real,
//! aligned, zeroed block this crate owns (D151). The blocks come from one fixed region,
//! bump-allocated in request order, so block n is the same address in every run and two runs
//! of one build behave identically (D584). The crate sits below every subsystem that hands out
//! handles. Blocks are never freed: a guest keeping a handle past its object's life reads
//! zeroes rather than a later object, and the count is bounded by what a title creates.

use std::sync::atomic::{AtomicU64, Ordering};

/// Where blocks handed to the guest live.
///
/// In the `0x0000_5E2*` family `docs/ADDRESS_MAP.md` keeps for regions of orbistoun's own
/// invention, so a stray handle is recognisable on sight.
pub const GUEST_BLOCK_BASE: u64 = 0x0000_5E2D_0000_0000;

/// How far the region reaches: sixty-four mebibytes, sized for a title that creates handles
/// in a loop. Exhaustion falls back rather than failing.
const SPAN: u64 = 64 * 1024 * 1024;

/// The next free address, or zero before the region is reserved.
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Reserves the region once, answering whether it is usable.
fn ready() -> bool {
    static READY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *READY.get_or_init(|| {
        let mut space = crate::AddressSpace::new();
        let placed = space
            .reserve(
                GUEST_BLOCK_BASE,
                SPAN,
                crate::Protection {
                    read: true,
                    write: true,
                    execute: false,
                },
            )
            .is_ok();
        if placed {
            // Leaked deliberately: the region outlives every handle into it, and the process
            // ends with the run.
            std::mem::forget(space);
            NEXT.store(GUEST_BLOCK_BASE, Ordering::Relaxed);
        }
        placed
    })
}

/// A zeroed, eight-byte-aligned block of `words` machine words, as an address.
///
/// Falls back to the host heap when the region is unusable or exhausted; such a run's handles
/// do not repeat, and [`repeat`] reports it.
#[must_use]
pub fn block(words: usize) -> u64 {
    let bytes = (words as u64).saturating_mul(8);
    if ready() {
        let at = NEXT.fetch_add(bytes, Ordering::Relaxed);
        if at.saturating_add(bytes) <= GUEST_BLOCK_BASE.saturating_add(SPAN) {
            return at;
        }
    }
    // A `u64` slice is eight-byte aligned, so a guest's word reads are aligned.
    let leaked: Box<[u64]> = vec![0_u64; words].into_boxed_slice();
    Box::leak(leaked).as_mut_ptr() as usize as u64
}

/// Whether the handles this run issued are the ones it would issue again.
#[must_use]
pub fn repeat() -> bool {
    ready()
}

/// How many bytes of the region have been handed out, for the report.
#[must_use]
pub fn taken() -> u64 {
    NEXT.load(Ordering::Relaxed)
        .saturating_sub(GUEST_BLOCK_BASE)
}

#[cfg(test)]
mod tests {
    /// Two blocks are never the same address, or a write to one corrupts the other.
    #[test]
    fn every_block_is_its_own() {
        let first = super::block(4);
        let second = super::block(4);
        assert_ne!(first, second, "two handles came back as one address");
        assert!(
            second >= first.saturating_add(32),
            "the second block starts inside the first"
        );
    }

    /// A block is eight-byte aligned, because a guest reads words out of it.
    #[test]
    fn a_block_is_word_aligned() {
        assert_eq!(super::block(2) % 8, 0, "a guest would read this unaligned");
    }

    /// A block is zeroed, so a field nobody set reads as null rather than as debris.
    #[test]
    fn a_block_starts_zeroed() {
        let at = super::block(4);
        // SAFETY: the address was just handed out by this module, is word-aligned, and nothing
        // else holds it.
        let first =
            unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at as usize)) };
        assert_eq!(first, 0, "a fresh block was not zeroed");
    }
}
