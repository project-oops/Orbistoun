//! An `mspace` created over a base serves from that range (D751).
//!
//! dlmalloc's `create_mspace_with_base(base, capacity, locked)` builds an arena inside memory the
//! caller supplies, and every allocation from it lies inside that memory. A guest that maps memory
//! the GPU sees and builds its graphics heap there as an mspace (PPSA21564's Onion pool:
//! `sceLibcMspaceCreate(0, 0x400000000, 0xb800000, 1)`) relies on that: a block from anywhere else
//! is memory the GPU was never given. An mspace created without a base stays on the shared heap
//! (D451).
//!
//! The bookkeeping is host-side, so nothing is written into the guest's region but what the guest
//! writes: a first-fit list of free spans and a map of live blocks, each block's start aligned as
//! asked and its length rounded to [`GRANULE`].

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// dlmalloc's minimum alignment on a 64-bit target, and the step block lengths are rounded to.
pub(crate) const GRANULE: u64 = 16;

/// One arena over a guest-supplied range.
#[derive(Debug)]
pub(crate) struct BasedSpace {
    /// Free spans, start to length, never adjacent.
    free: BTreeMap<u64, u64>,
    /// Live blocks, start to their length and the size asked for, which is what
    /// `sceLibcMspaceMallocUsableSize` answers, as the platform's does for its own heap.
    live: BTreeMap<u64, (u64, u64)>,
}

impl BasedSpace {
    /// An arena over `capacity` bytes at `base`, all of it free.
    pub(crate) fn new(base: u64, capacity: u64) -> Self {
        let mut free = BTreeMap::new();
        if capacity > 0 {
            free.insert(base, capacity);
        }
        Self {
            free,
            live: BTreeMap::new(),
        }
    }

    /// Takes `size` bytes at a multiple of `align` (a power of two, at least [`GRANULE`]), first
    /// fit; `None` when no free span holds it.
    pub(crate) fn take(&mut self, size: u64, align: u64) -> Option<u64> {
        let align = align.max(GRANULE);
        if !align.is_power_of_two() {
            return None;
        }
        let len = size.max(1).checked_next_multiple_of(GRANULE)?;
        let (span_start, span_len, start) = self.free.iter().find_map(|(&at, &span)| {
            let start = at.checked_next_multiple_of(align)?;
            let end = start.checked_add(len)?;
            (end <= at.checked_add(span)?).then_some((at, span, start))
        })?;
        self.free.remove(&span_start);
        if start > span_start {
            self.free.insert(span_start, start - span_start);
        }
        let tail = span_start + span_len - (start + len);
        if tail > 0 {
            self.free.insert(start + len, tail);
        }
        self.live.insert(start, (len, size));
        Some(start)
    }

    /// Gives back the block starting at `at`, merging it with free neighbours; `false` for an
    /// address no live block starts at.
    pub(crate) fn give_back(&mut self, at: u64) -> bool {
        let Some((block, _)) = self.live.remove(&at) else {
            return false;
        };
        let (mut start, mut end) = (at, at + block);
        if let Some(after) = self.free.remove(&end) {
            end += after;
        }
        if let Some((&before, &before_len)) = self.free.range(..at).next_back()
            && before + before_len == at
        {
            self.free.remove(&before);
            start = before;
        }
        self.free.insert(start, end - start);
        true
    }

    /// The size asked for the live block starting at `at`.
    pub(crate) fn length_of(&self, at: u64) -> Option<u64> {
        self.live.get(&at).map(|&(_, asked)| asked)
    }
}

/// Every arena created over a base, by the handle the guest was given.
fn spaces() -> &'static Mutex<BTreeMap<u64, BasedSpace>> {
    static SPACES: Mutex<BTreeMap<u64, BasedSpace>> = Mutex::new(BTreeMap::new());
    &SPACES
}

/// Records `handle` as an arena over `capacity` bytes at `base`.
pub(crate) fn create(handle: u64, base: u64, capacity: u64) {
    spaces()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(handle, BasedSpace::new(base, capacity));
}

/// Forgets the arena `handle`, if it is one.
pub(crate) fn destroy(handle: u64) {
    spaces()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&handle);
}

/// An allocation from the arena `handle`: `None` when `handle` is not an arena created over a
/// base (the shared heap answers it), `Some(0)` when the arena is full.
pub(crate) fn allocate_in(handle: u64, size: u64, align: u64) -> Option<u64> {
    let mut spaces = spaces().lock().unwrap_or_else(PoisonError::into_inner);
    let space = spaces.get_mut(&handle)?;
    Some(space.take(size, align).unwrap_or(0))
}

/// Releases `pointer` to whichever arena holds it; `false` when none does.
pub(crate) fn release(pointer: u64) -> bool {
    spaces()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values_mut()
        .any(|space| space.give_back(pointer))
}

/// The length of `pointer`'s block when an arena holds it.
pub(crate) fn length_of(pointer: u64) -> Option<u64> {
    spaces()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .find_map(|space| space.length_of(pointer))
}

#[cfg(test)]
mod tests {
    use super::{BasedSpace, GRANULE};

    /// Every block lies inside the range and at its alignment, a full arena answers none, and a
    /// freed block is reused and merged back: the Onion pool's 184 MiB at 0x400000000.
    #[test]
    fn blocks_stay_inside_the_range_at_their_alignment() {
        let (base, capacity) = (0x4_0000_0000_u64, 0xb80_0000_u64);
        let mut space = BasedSpace::new(base, capacity);
        let small = space.take(0x30, 0x10).expect("small");
        assert_eq!(small, base);
        let big = space.take(0x20_0000, 0x1_0000).expect("2 MiB at 64 KiB");
        assert_eq!(big % 0x1_0000, 0);
        assert!(big >= base && big + 0x20_0000 <= base + capacity);
        assert!(space.take(capacity, GRANULE).is_none(), "more than is left");
        assert!(space.give_back(big));
        assert!(!space.give_back(big), "a block is given back once");
        assert!(space.give_back(small));
        assert_eq!(
            space.take(capacity, GRANULE),
            Some(base),
            "everything merged back into one span"
        );
    }
}
