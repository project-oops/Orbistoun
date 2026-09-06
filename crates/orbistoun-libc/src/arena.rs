//! A heap at an address the host did not choose.
//!
//! # The question this exists to answer
//!
//! A run of PPSA02664 makes either 2077 calls across 44 distinct functions or 2080 across
//! 46, and which one it makes changes between runs of the same binary against the same
//! title. D499 established that this is a **branch and not noise** - the two outcomes are
//! internally consistent, roughly forty/sixty, and no amount of averaging moves either one.
//!
//! Five candidates were killed by measurement: a persisted trace, a retained sandbox,
//! uninitialised heap contents, uninitialised stack contents, and uninitialised direct
//! memory. One survived, and it is structural rather than incidental:
//!
//! > `malloc` is served from `std::alloc` (D128), so every pointer the guest receives is a
//! > **host** address, and the host randomises its layout on every run.
//!
//! A guest that hashes a pointer, buckets one, compares two, or aligns against one is then
//! reading a number that differs run to run - and a branch taken on that number is exactly
//! the shape D499 measured. This module makes those addresses fixed so the hypothesis can
//! be tested rather than argued.
//!
//! # It is off unless asked for, and it says whether it fired
//!
//! `ORBISTOUN_HEAP_BASE` is a diagnostic that **intervenes**: it changes the program under
//! observation, so principle 3 applies in full. It is off by default, and a run that asked
//! for it gets a line saying what it served - because a fixed heap that quietly fell back
//! to `std::alloc` and a fixed heap that served everything produce identical call counts,
//! and the difference is the entire finding (D325).
//!
//! # What it deliberately does not do
//!
//! **It never reuses a freed block.** A bump pointer with a no-op `free` is a few lines and
//! is obviously correct; a free list is a second allocator, and a second allocator with a
//! bug in it would produce a *different* nondeterminism while claiming to remove one. The
//! region is sized for a run that walls in the first few thousand calls, and exhaustion
//! falls back to the host heap and is **counted and reported** rather than hidden - a run
//! that fell back has not tested the hypothesis, and must not be read as though it had.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use orbistoun_mem::{AddressSpace, Protection};

/// Where to put the region, when the variable does not say.
///
/// # Not a free choice, and the first guess was wrong
///
/// The first attempt took `0x7400_0000_0000` on the reasoning that it was clear of the
/// loader's module base and of the thunk table. It is the **kernel's mapping arena**
/// (`orbistoun_kernel::MAPPING_BASE`), which `sceKernelReserveVirtualRange` hands out from,
/// so the guest asked for an address this region already held. Sixteen reservations were
/// refused that would otherwise have succeeded and the run fell from 2077 calls to 219.
///
/// So it goes in the family this project already keeps for regions of its own invention -
/// `0x0000_5E2*_0000_0000`, alongside the sentinel, content, unserved-global, poison and
/// described-object bases - which is the one range nothing else claims and every reader of
/// those constants already recognises on sight.
pub(crate) const DEFAULT_BASE: u64 = 0x0000_5E2C_0000_0000;

/// How far the region reaches.
///
/// Sixty-four mebibytes, a multiple of the guest page size, which the reservation requires.
/// Chosen against the run it exists to measure rather than against any guest's needs: the
/// wall arrives inside three thousand calls, and no run has allocated a thousandth of this
/// before reaching it. Exhaustion is a reported fallback and not a failure, so a guest that
/// outgrows it degrades to today's behaviour rather than to a null return.
const SPAN: u64 = 64 * 1024 * 1024;

/// The reservation, and where the next block starts inside it.
struct Arena {
    /// The first address in the region.
    base: u64,
    /// One past the last.
    end: u64,
    /// The next free address. Moves forward, or backward when `descending`.
    cursor: AtomicU64,
    /// Hand out each block *below* the last instead of above it.
    ///
    /// # The one thing this separates
    ///
    /// Fifty-one runs across thirteen bases established that the oscillation stops when the
    /// heap is fixed and that **the address value is not what stops it** - thirteen
    /// different addresses, thirty bits apart, all took the same branch. What a bump region
    /// also guarantees, and the host allocator does not, is that later blocks are at
    /// *higher* addresses than earlier ones: the host heap reuses freed blocks and can hand
    /// back one below a block it gave out earlier.
    ///
    /// So this reverses exactly that one property and holds everything else - fixed,
    /// deterministic, contiguous, never reused. A guest comparing two pointers, ordering
    /// them, or keying a container on them sees the opposite answer; a guest reading their
    /// values sees a different but equally fixed set. If the branch flips, the mechanism is
    /// the **ordering**, and nothing else here could have told them apart (D513).
    descending: bool,
    /// Held for the lifetime of the process: [`orbistoun_mem::Reservation`] releases the
    /// mapping when it drops, and a heap that unmapped itself while the guest still held
    /// pointers into it would fault somewhere with no connection to this file.
    _space: AddressSpace,
}

/// Built once, on the first allocation, and [`None`] whenever the variable is unset or the
/// reservation was refused.
static ARENA: OnceLock<Option<Arena>> = OnceLock::new();

/// Why the region does not exist, when a base was asked for and could not be had.
///
/// Separate from the absent-variable case on purpose. "Not asked for" and "asked for and
/// refused" are the same silence in a run report, and only one of them is a result.
static REFUSAL: OnceLock<String> = OnceLock::new();

/// Blocks served from the region, and bytes.
static SERVED: AtomicU64 = AtomicU64::new(0);
/// Bytes served, alongside [`SERVED`].
static SERVED_BYTES: AtomicU64 = AtomicU64::new(0);
/// Requests the region could not satisfy, which went to the host heap instead.
static FELL_BACK: AtomicU64 = AtomicU64::new(0);

/// The region, building it on first use.
fn arena() -> Option<&'static Arena> {
    ARENA.get_or_init(build).as_ref()
}

/// Reads the variable and takes the reservation, or records why it could not.
fn build() -> Option<Arena> {
    let raw = orbistoun_env::HEAP_BASE.get()?;
    let text = raw.trim();
    // A `-down` suffix on either form reverses the direction blocks are handed out in.
    let (text, descending) = match text.strip_suffix("-down") {
        Some(head) => (head, true),
        None => (text, false),
    };
    if text.eq_ignore_ascii_case("default") {
        return reserve(DEFAULT_BASE, descending);
    }
    let Ok(base) = u64::from_str_radix(text.trim_start_matches("0x"), 16) else {
        let _ = REFUSAL.set(format!(
            "fixed heap base {text:?} is not a hexadecimal address - the host heap was used"
        ));
        return None;
    };
    reserve(base, descending)
}

/// Takes the region at `base`, or records why it could not be had.
fn reserve(base: u64, descending: bool) -> Option<Arena> {
    let mut space = AddressSpace::new();
    match space.reserve(base, SPAN, Protection::READ_WRITE) {
        Ok(_) => Some(Arena {
            base,
            end: base.saturating_add(SPAN),
            cursor: AtomicU64::new(if descending {
                base.saturating_add(SPAN)
            } else {
                base
            }),
            descending,
            _space: space,
        }),
        Err(error) => {
            // The address, not just the error: a base the host will not give is the one
            // thing a reader can act on, and it is the caller's own input.
            let _ = REFUSAL.set(format!(
                "fixed heap at {base:#x} could not be reserved ({error}) - the host heap was used"
            ));
            None
        }
    }
}

/// Takes `total` bytes aligned to `align`, or [`None`] if the region cannot serve them.
///
/// The block it returns is the same shape [`crate::allocate`] builds on the host heap - a
/// base aligned to `align`, `total` bytes long - so the header, `free` and `realloc` need
/// no knowledge of which allocator answered.
pub(crate) fn take(total: usize, align: usize) -> Option<u64> {
    let arena = arena()?;
    let (Ok(total), Ok(align)) = (u64::try_from(total), u64::try_from(align)) else {
        return None;
    };
    // Where a block starting from `cursor` would begin, in whichever direction this region
    // runs. A pure function of the cursor, so the update below and the recomputation after
    // it agree by construction rather than by being kept in step.
    let start_from = |cursor: u64| -> Option<u64> {
        if arena.descending {
            // Down: take the space *below* the cursor, then round the start down to the
            // alignment - which can only move it further down, so it stays inside.
            let start = (cursor.checked_sub(total)? / align) * align;
            (start >= arena.base).then_some(start)
        } else {
            let start = cursor.checked_next_multiple_of(align)?;
            (start.checked_add(total)? <= arena.end).then_some(start)
        }
    };
    let moved = arena
        .cursor
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |cursor| {
            let start = start_from(cursor)?;
            if arena.descending {
                Some(start)
            } else {
                start.checked_add(total)
            }
        })
        .ok();
    let Some(previous) = moved else {
        FELL_BACK.fetch_add(1, Ordering::Relaxed);
        return None;
    };
    let start = start_from(previous)?;
    SERVED.fetch_add(1, Ordering::Relaxed);
    SERVED_BYTES.fetch_add(total, Ordering::Relaxed);
    Some(start)
}

/// Whether `address` is inside the region.
///
/// `free` asks before it does anything else, because the two allocators must never be
/// crossed: `std::alloc::dealloc` against memory this module reserved is undefined
/// behaviour, and the header a block carries is identical either way, so the header cannot
/// be what distinguishes them.
pub(crate) fn holds(address: u64) -> bool {
    arena().is_some_and(|a| address >= a.base && address < a.end)
}

/// One line for a run report: what the fixed heap actually did.
///
/// [`None`] when none was asked for, so a quiet run stays quiet.
pub(crate) fn summary() -> Option<String> {
    orbistoun_env::HEAP_BASE.get()?;
    // Force the build, so a refusal is reported even by a run that never allocated.
    let region = arena();
    if let Some(reason) = REFUSAL.get() {
        return Some(reason.clone());
    }
    let region = region?;
    let served = SERVED.load(Ordering::Relaxed);
    let bytes = SERVED_BYTES.load(Ordering::Relaxed);
    let spilled = FELL_BACK.load(Ordering::Relaxed);
    if served == 0 {
        // The sentence a reader must never have to infer: the diagnostic was asked for and
        // did nothing, so the run tested nothing.
        return Some(format!(
            "fixed heap at {:#x} reserved and never used - nothing was tested",
            region.base
        ));
    }
    let mut line = format!(
        "fixed heap at {:#x}: {served} allocation(s), {bytes} bytes",
        region.base
    );
    if spilled > 0 {
        // Stated in the same line as the successes, because a partial result read as a
        // whole one is the failure this whole diagnostic exists to avoid.
        use std::fmt::Write as _;
        let _ = write!(
            line,
            " - {spilled} spilled to the host heap, so this run did NOT hold every address fixed"
        );
    }
    Some(line)
}
