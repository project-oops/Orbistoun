//! An `mspace` created over a base serves from that range (D751).
//!
//! dlmalloc's `create_mspace_with_base(base, capacity, locked)` builds an arena inside memory the
//! caller supplies, and every allocation from it lies inside that memory. A guest that maps memory
//! the GPU sees and builds its graphics heap there as an mspace (PPSA21564's Onion pool:
//! `sceLibcMspaceCreate(0, 0x400000000, 0xb800000, 1)`) relies on that: a block from anywhere else
//! is memory the GPU was never given. An mspace created without a base stays on the shared heap
//! (D451).
//!
//! The bookkeeping is host-side: a first-fit list of free spans and a map of live blocks, each
//! block's start aligned as asked and its length rounded to [`GRANULE`]. The one thing written into
//! the guest's region is what dlmalloc keeps in front of every block it hands out, its chunk header,
//! because a guest reads it (D771): PPSA21564's allocator wrapper reads the word before each block,
//! masks its low bits and counts the rest as the bytes the block takes.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// dlmalloc's minimum alignment on a 64-bit target, and the step block lengths are rounded to.
pub(crate) const GRANULE: u64 = 16;

/// The chunk header dlmalloc keeps in front of a block: `prev_foot`, then `head` (D771).
pub(crate) const HEADER: u64 = 16;

/// `head`'s flag bits for a block in use whose predecessor is in use too: `CINUSE_BIT | PINUSE_BIT`.
const IN_USE: u64 = 3;

/// One arena over a guest-supplied range.
#[derive(Debug)]
pub(crate) struct BasedSpace {
    /// The bytes the arena was created over.
    capacity: u64,
    /// The bytes live blocks take now, and the most they have taken.
    in_use: u64,
    peak: u64,
    /// Free spans, start to length, never adjacent.
    free: BTreeMap<u64, u64>,
    /// Live blocks, by the address handed out, to their length, the size asked for (which is what
    /// `sceLibcMspaceMallocUsableSize` answers, as the platform's does for its own heap), and where
    /// their header starts.
    live: BTreeMap<u64, (u64, u64, u64)>,
}

impl BasedSpace {
    /// An arena over `capacity` bytes at `base`, all of it free.
    pub(crate) fn new(base: u64, capacity: u64) -> Self {
        let mut free = BTreeMap::new();
        if capacity > 0 {
            free.insert(base, capacity);
        }
        Self {
            capacity,
            in_use: 0,
            peak: 0,
            free,
            live: BTreeMap::new(),
        }
    }

    /// Takes `size` bytes at a multiple of `align` (a power of two, at least [`GRANULE`]), first
    /// fit, with [`HEADER`] bytes in front of it; `None` when no free span holds both.
    ///
    /// Answers the address handed out, and the header's start and the chunk size `head` states.
    pub(crate) fn take(&mut self, size: u64, align: u64) -> Option<(u64, u64, u64)> {
        let align = align.max(GRANULE);
        if !align.is_power_of_two() {
            return None;
        }
        let len = size.max(1).checked_next_multiple_of(GRANULE)?;
        let (span_start, span_len, start) = self.free.iter().find_map(|(&at, &span)| {
            let start = at.checked_add(HEADER)?.checked_next_multiple_of(align)?;
            let end = start.checked_add(len)?;
            (end <= at.checked_add(span)?).then_some((at, span, start))
        })?;
        let chunk = start - HEADER;
        self.free.remove(&span_start);
        if chunk > span_start {
            self.free.insert(span_start, chunk - span_start);
        }
        let tail = span_start + span_len - (start + len);
        if tail > 0 {
            self.free.insert(start + len, tail);
        }
        self.live.insert(start, (len, size, chunk));
        self.in_use += len + HEADER;
        self.peak = self.peak.max(self.in_use);
        Some((start, chunk, len + HEADER))
    }

    /// Gives back the block starting at `at`, merging it with free neighbours; `false` for an
    /// address no live block starts at.
    pub(crate) fn give_back(&mut self, at: u64) -> bool {
        let Some((block, _, chunk)) = self.live.remove(&at) else {
            return false;
        };
        let (mut start, mut end) = (chunk, at + block);
        self.in_use -= end - start;
        if let Some(after) = self.free.remove(&end) {
            end += after;
        }
        if let Some((&before, &before_len)) = self.free.range(..chunk).next_back()
            && before + before_len == chunk
        {
            self.free.remove(&before);
            start = before;
        }
        self.free.insert(start, end - start);
        true
    }

    /// The size asked for the live block starting at `at`.
    pub(crate) fn length_of(&self, at: u64) -> Option<u64> {
        self.live.get(&at).map(|&(_, asked, _)| asked)
    }
}

/// What an arena reports of itself: its capacity, the bytes in use and the most ever in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Usage {
    pub(crate) capacity: u64,
    pub(crate) in_use: u64,
    pub(crate) peak: u64,
}

/// The usage of the arena `handle`, when it is one created over a base.
pub(crate) fn usage(handle: u64) -> Option<Usage> {
    spaces()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&handle)
        .map(|space| Usage {
            capacity: space.capacity,
            in_use: space.in_use,
            peak: space.peak,
        })
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
///
/// The block's dlmalloc header is written in front of it: `prev_foot` zero and `head` its chunk size
/// with both in-use bits (D771).
pub(crate) fn allocate_in(handle: u64, size: u64, align: u64) -> Option<u64> {
    let mut spaces = spaces().lock().unwrap_or_else(PoisonError::into_inner);
    let space = spaces.get_mut(&handle)?;
    let Some((start, chunk, chunk_size)) = space.take(size, align) else {
        return Some(0);
    };
    // SAFETY: the header lies inside the range the guest created the arena over, which it mapped
    // for this arena, and in front of a block no one else holds.
    let written = unsafe { orbistoun_mem::guest::write_u64(chunk, 0) }
        // SAFETY: as above.
        && unsafe { orbistoun_mem::guest::write_u64(chunk + 8, chunk_size | IN_USE) };
    if !written {
        space.give_back(start);
        return Some(0);
    }
    Some(start)
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
    use super::{BasedSpace, GRANULE, HEADER};

    /// Every block lies inside the range and at its alignment, with its header in front of it; a
    /// full arena answers none, and a freed block is reused and merged back: the Onion pool's
    /// 184 MiB at 0x400000000.
    #[test]
    fn blocks_stay_inside_the_range_at_their_alignment() {
        let (base, capacity) = (0x4_0000_0000_u64, 0xb80_0000_u64);
        let mut space = BasedSpace::new(base, capacity);
        let (small, chunk, size) = space.take(0x30, 0x10).expect("small");
        assert_eq!((small, chunk, size), (base + HEADER, base, 0x30 + HEADER));
        let (big, big_chunk, _) = space.take(0x20_0000, 0x1_0000).expect("2 MiB at 64 KiB");
        assert_eq!(big % 0x1_0000, 0);
        assert!(big_chunk >= base + 0x40 && big + 0x20_0000 <= base + capacity);
        assert!(space.take(capacity, GRANULE).is_none(), "more than is left");
        assert!(space.give_back(big));
        assert!(!space.give_back(big), "a block is given back once");
        assert!(space.give_back(small));
        assert_eq!(
            space
                .take(capacity - HEADER, GRANULE)
                .map(|(_, chunk, _)| chunk),
            Some(base),
            "everything merged back into one span"
        );
    }

    /// The word in front of a block states its chunk size with the in-use bits, as dlmalloc's does,
    /// so a guest masking the low bits and taking the header off counts the block's length (D771).
    #[test]
    fn a_block_carries_its_dlmalloc_header() {
        let words = 64;
        let base = orbistoun_mem::blocks::block(words);
        let handle = 0x7e57_0000_0000_0001;
        super::create(handle, base, words as u64 * 8);
        let block = super::allocate_in(handle, 0x30, GRANULE).expect("an arena");
        assert_eq!(block, base + HEADER);
        // SAFETY: inside the arena just created over the block.
        let head = unsafe { orbistoun_mem::guest::read_u64(block - 8) }.expect("readable");
        assert_eq!(head & !0xf, 0x30 + HEADER);
        assert_eq!(head & 0x3, 0x3);
        assert_eq!((head & 0xffff_fff0) - 0x10, 0x30, "what the guest counts");
        super::destroy(handle);
    }
}
