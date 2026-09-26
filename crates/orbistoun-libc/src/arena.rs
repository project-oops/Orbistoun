//! A heap at an address the host did not choose.
//!
//! `malloc` is served from `std::alloc` (D128), so every pointer the guest receives is a host
//! address that the host randomises per run; a guest that hashes, orders or aligns against a
//! pointer can then branch differently run to run. `ORBISTOUN_HEAP_BASE` serves allocations
//! from a fixed region instead, so that hypothesis can be tested. It intervenes in the program
//! under observation, so it is off by default and a run that asked for it reports what it
//! served (D227). It never reuses a freed block: a bump pointer with a no-op `free` cannot add
//! nondeterminism of its own. Exhaustion falls back to the host heap and is counted.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use orbistoun_mem::{AddressSpace, Protection};

/// Where to put the region, when the variable does not say.
///
/// In the `0x0000_5E2*_0000_0000` family this project keeps for regions of its own invention,
/// the one range nothing else claims. The kernel's mapping arena
/// (`orbistoun_kernel::MAPPING_BASE`) is where `sceKernelReserveVirtualRange` hands out from,
/// so the heap must stay clear of it.
pub(crate) const DEFAULT_BASE: u64 = 0x0000_5E2C_0000_0000;

/// How far the region reaches.
///
/// Sixty-four mebibytes, a multiple of the guest page size, which the reservation requires.
/// Exhaustion is a reported fallback to the host heap, not a null return.
const SPAN: u64 = 64 * 1024 * 1024;

/// The reservation, and where the next block starts inside it.
struct Arena {
    /// The first address in the region.
    base: u64,
    /// One past the last.
    end: u64,
    /// The next free address. Moves forward, or backward when `descending`.
    cursor: AtomicU64,
    /// Hand out each block below the last instead of above it.
    ///
    /// A bump region guarantees later blocks are at higher addresses; the host heap does not,
    /// since it reuses freed blocks. This reverses only that property and keeps the region fixed,
    /// deterministic, contiguous and never reused, so a guest branch that flips with it depends on
    /// pointer ordering rather than on pointer values.
    descending: bool,
    /// Held for the lifetime of the process: [`orbistoun_mem::Reservation`] releases the mapping
    /// when it drops, and the guest still holds pointers into it.
    _space: AddressSpace,
}

/// Built once, on the first allocation, and [`None`] whenever the variable is unset or the
/// reservation was refused.
static ARENA: OnceLock<Option<Arena>> = OnceLock::new();

/// Why the region does not exist, when a base was asked for and could not be had.
///
/// Separate from the absent-variable case, so "asked for and refused" is not reported as
/// "not asked for".
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
            // The address, not just the error: a base the host will not give is the caller's own
            // input, and the one thing a reader can act on.
            let _ = REFUSAL.set(format!(
                "fixed heap at {base:#x} could not be reserved ({error}) - the host heap was used"
            ));
            None
        }
    }
}

/// Takes `total` bytes aligned to `align`, or [`None`] if the region cannot serve them.
///
/// The block has the same shape [`crate::allocate`] builds on the host heap, so the header,
/// `free` and `realloc` need no knowledge of which allocator answered.
pub(crate) fn take(total: usize, align: usize) -> Option<u64> {
    let arena = arena()?;
    let (Ok(total), Ok(align)) = (u64::try_from(total), u64::try_from(align)) else {
        return None;
    };
    // Where a block starting from `cursor` would begin, in this region's direction. A pure
    // function of the cursor, so the update below and the recomputation after it agree.
    let start_from = |cursor: u64| -> Option<u64> {
        if arena.descending {
            // Down: take the space below the cursor, then round the start down to the alignment,
            // which keeps it inside.
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
/// `free` asks first, because the two allocators must never be crossed: `std::alloc::dealloc`
/// on this region's memory is undefined behaviour, and the block header is identical either
/// way.
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
        // The diagnostic was asked for and did nothing, so the run tested nothing.
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
        // Fallbacks are stated in the same line as the successes, so a partial result does not
        // read as a whole one.
        use std::fmt::Write as _;
        let _ = write!(
            line,
            " - {spilled} spilled to the host heap, so this run did NOT hold every address fixed"
        );
    }
    Some(line)
}
