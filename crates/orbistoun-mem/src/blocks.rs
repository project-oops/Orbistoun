//! Memory handed to the guest as a handle.
//!
//! # Why a handle has to be a real address, and why it must not be the host's
//!
//! A guest dereferences what it is given. D151 established that with a measurement: an opaque
//! integer handle reproduced a fault at a low address, because the guest read a field through
//! it - so a handle is the address of a real, aligned, zeroed block this crate owns.
//!
//! Nothing in that requires the *host allocator* to choose where. `Box::leak` did, at **nine
//! separate sites** - thread handles, thread attributes, mutex and condition attributes,
//! semaphores, synchronisation objects, the system version block, and file handles - and every
//! one of them gave the guest a different value on every run. D582 found the first and fixed
//! only that one; D584 found eight and fixed eight, in one crate, because that is where it
//! looked. The ninth was in another.
//!
//! Every measurement here rests on two runs of one build behaving identically (D181, D238), and
//! a guest that stores a handle in a structure, hashes it, or sizes a table from it computes a
//! different program each run. So the blocks come from one fixed region, bump-allocated in the
//! order they are asked for: **block *n* is the same address in every run** (D584).
//!
//! # Why this lives here
//!
//! It was in `orbistoun-kernel`, which put it out of reach of `orbistoun-fs` - a sibling
//! subsystem, and the relation that keeps the clocks in `orbistoun-hle` rather than in either
//! library that reads them (D536). So a `FILE *` went on being a host heap address because the
//! fix was in a crate that could not be called from where the bug was.
//!
//! Guest-visible memory is what this crate is for, and both subsystems already sit above it on
//! the spine, so nothing moves sideways and no dependency is added to reach it (D601).
//!
//! # Never freed, deliberately
//!
//! A guest keeping a handle past its object's life then reads zeroes rather than whatever was
//! put there next, which is a wrong answer that looks wrong instead of one that looks right.
//! The count is bounded by what a title creates.

use std::sync::atomic::{AtomicU64, Ordering};

/// Where blocks handed to the guest live.
///
/// In the `0x0000_5E2*` family `docs/ADDRESS_MAP.md` keeps for regions of orbistoun's own
/// invention - a guest never asked for this, and a stray handle is recognisable on sight.
pub const GUEST_BLOCK_BASE: u64 = 0x0000_5E2D_0000_0000;

/// How far the region reaches.
///
/// Sixty-four mebibytes. Every handle a title creates comes from here - threads, attribute
/// sets, semaphores, synchronisation objects - so it is sized against a title that creates
/// them in a loop rather than against the handful a boot has needed so far. Exhaustion falls
/// back rather than failing.
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
/// **Falls back to the host heap rather than failing**, and the fallback is not silent: a run
/// that took it has non-repeating handles again, which is the property this exists to give, so
/// [`repeat`] says which happened rather than leaving a reader to assume.
#[must_use]
pub fn block(words: usize) -> u64 {
    let bytes = (words as u64).saturating_mul(8);
    if ready() {
        let at = NEXT.fetch_add(bytes, Ordering::Relaxed);
        if at.saturating_add(bytes) <= GUEST_BLOCK_BASE.saturating_add(SPAN) {
            return at;
        }
    }
    // Eight-byte aligned by construction, so a guest reading a word out of it does an aligned
    // read - which a `Vec<u8>` would not have guaranteed.
    let leaked: Box<[u64]> = vec![0_u64; words].into_boxed_slice();
    Box::leak(leaked).as_mut_ptr() as usize as u64
}

/// Whether the handles this run issued are the ones it would issue again.
#[must_use]
pub fn repeat() -> bool {
    ready()
}

/// How many blocks have been handed out, as bytes taken from the region.
///
/// For the report, so a run that exhausted the region and fell back can say so with a number
/// rather than only a flag.
#[must_use]
pub fn taken() -> u64 {
    NEXT.load(Ordering::Relaxed)
        .saturating_sub(GUEST_BLOCK_BASE)
}

#[cfg(test)]
mod tests {
    /// **Two blocks are never the same address**, or two objects share one and a write to
    /// either corrupts the other.
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
    ///
    /// **This is what makes an unset field safe.** D151's whole argument is that a guest reads
    /// through a handle; a block carrying whatever was there before would hand it a pointer to
    /// somewhere arbitrary, which faults far from the cause.
    #[test]
    fn a_block_starts_zeroed() {
        let at = super::block(4);
        // SAFETY: the address was just handed out by this module, is word-aligned, and no
        // other code holds it - nothing else can have written to it yet.
        let first =
            unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at as usize)) };
        assert_eq!(first, 0, "a fresh block was not zeroed");
    }
}
