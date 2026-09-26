//! Direct memory: the physical address space a guest allocates out of.
//!
//! The target exposes its memory as a flat physical range that a guest carves up itself:
//! reserve a span, then map it into the virtual address space separately. Guests walk this map
//! with `sceKernelDirectMemoryQuery` before anything else, so the walk must terminate.
//!
//! The model is a sorted list of non-overlapping regions covering the whole range, each free
//! or taken. Query walks it, allocation splits a free region, release merges neighbours. Memory
//! types are recorded and otherwise ignored, since nothing consumes them.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The default total direct memory a guest may allocate from: the retail figure (D697).
///
/// On the hardware `sceKernelGetDirectMemorySize` answers `0x3_0000_0000` (12 GiB) to a retail
/// title and `0x1_4000_0000` (5 GiB) to a homebrew payload (obSCEne `020-memory/direct-size`);
/// the sandbox hands the two classes different budgets. A homebrew leg sets
/// [`Settings::pool_bytes`] to [`HOMEBREW_DIRECT_MEMORY_SIZE`]. A guest walks the map against
/// the reported size, so the reported size and the pool come from one setting.
pub const DIRECT_MEMORY_SIZE: u64 = 0x3_0000_0000;

/// The direct-memory pool a homebrew or conformance guest is handed, five gibibytes.
///
/// A homebrew leg sets [`Settings::pool_bytes`] to it; the default is the retail
/// [`DIRECT_MEMORY_SIZE`].
pub const HOMEBREW_DIRECT_MEMORY_SIZE: u64 = 0x1_4000_0000;

/// The flexible memory available at launch, before the guest maps any (D444).
///
/// obSCEne's `020-memory/flexible-available` answers `0x1b40_0000` (about 437 MiB) on the
/// hardware. It is the system default: titles carry an empty `PT_SCE_PROCPARAM` mem-param, so
/// none overrides it. Flexible memory is a separate budget from the direct pool; `available`
/// is this figure minus what the guest has mapped ([`flexible_available`]).
pub const FLEXIBLE_MEMORY_SIZE: u64 = 0x1b40_0000;

/// The configured flexible-memory total, the ceiling the available figure counts down from.
///
/// obSCEne's `020-memory/flexible-configured` answers `0x1c00_0000` (448 MiB) on the hardware,
/// `0xc0_0000` above the available figure: the share the system has mapped at launch.
pub const FLEXIBLE_CONFIGURED: u64 = 0x1c00_0000;

/// Flexible-memory bytes the guest has mapped, so [`flexible_available`] falls as memory is
/// mapped.
static FLEXIBLE_MAPPED: AtomicU64 = AtomicU64::new(0);

/// The configured flexible-memory total, which does not move as memory is mapped.
pub fn flexible_configured() -> u64 {
    FLEXIBLE_CONFIGURED
}

/// The flexible memory available to map now: the launch figure minus what the guest has mapped.
pub fn flexible_available() -> u64 {
    FLEXIBLE_MEMORY_SIZE.saturating_sub(FLEXIBLE_MAPPED.load(Ordering::Relaxed))
}

/// Records a flexible mapping of `len` bytes against the budget.
pub fn record_flexible_map(len: u64) {
    FLEXIBLE_MAPPED.fetch_add(len, Ordering::Relaxed);
}

/// Returns `len` bytes to the flexible budget on release, saturating so an unmatched release
/// cannot hand back more than was taken.
pub fn record_flexible_release(len: u64) {
    let _ = FLEXIBLE_MAPPED.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |mapped| {
        Some(mapped.saturating_sub(len))
    });
}

/// Resets the flexible-memory budget, so one test does not see another's mappings.
#[cfg(test)]
pub fn reset_flexible() {
    FLEXIBLE_MAPPED.store(0, Ordering::Relaxed);
}

/// Alignment every direct-memory boundary satisfies.
///
/// Direct memory is handed out in 16 KiB units, which [`orbistoun_core::DIRECT_MEMORY_ALIGN`]
/// also records for the address-space layer.
pub const DIRECT_ALIGN: u64 = orbistoun_core::DIRECT_MEMORY_ALIGN;

/// One span of the physical range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// First physical address.
    pub start: u64,
    /// One past the last.
    pub end: u64,
    /// Whether a guest has taken it.
    pub allocated: bool,
    /// The memory type the guest asked for, recorded and otherwise unused.
    pub memory_type: u32,
}

impl Region {
    /// Length in bytes.
    pub const fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Whether the region covers nothing.
    pub const fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// The physical range, and what has been taken from it.
#[derive(Debug)]
pub struct DirectMemory {
    regions: Vec<Region>,
}

impl Default for DirectMemory {
    fn default() -> Self {
        Self::new(DIRECT_MEMORY_SIZE)
    }
}

impl DirectMemory {
    /// An empty physical range of `size` bytes.
    pub fn new(size: u64) -> Self {
        Self::with_shape(size, MapShape::Whole)
    }

    /// A range laid out the way `shape` describes.
    ///
    /// Separate from [`Self::new`], which is the plain whole-range layout.
    pub fn with_shape(size: u64, shape: MapShape) -> Self {
        Self {
            regions: shape.regions(size),
        }
    }

    /// Every region, in address order.
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// The region containing `offset`, or the first one after it.
    ///
    /// A guest walks by handing back the end of what it last saw, so a query that only answered
    /// for offsets inside a region would stall the walk at the first gap and the guest would repeat
    /// it indefinitely.
    pub fn query(&self, offset: u64) -> Option<Region> {
        self.regions
            .iter()
            .find(|r| r.end > offset && !r.is_empty())
            .copied()
    }

    /// Takes `len` bytes, searching from `search_start`.
    ///
    /// Returns the physical address, or `None` when nothing large enough is free. First fit: the
    /// guest decides placement policy.
    pub fn allocate(&mut self, search_start: u64, len: u64, memory_type: u32) -> Option<u64> {
        // Checked, not panicking: this is reachable from a guest call (D156).
        let len = len.checked_next_multiple_of(DIRECT_ALIGN)?;
        if len == 0 {
            return None;
        }
        let index = self
            .regions
            .iter()
            .position(|r| !r.allocated && r.end.saturating_sub(r.start.max(search_start)) >= len)?;

        let region = self.regions[index];
        let start = region
            .start
            .max(search_start)
            .checked_next_multiple_of(DIRECT_ALIGN)?;
        let end = start.checked_add(len)?;
        if end > region.end {
            return None;
        }

        // Replace the free region with up to three: the untouched head, the new allocation, and the
        // untouched tail. Empty ones are dropped, because a zero-length region would stall a walk.
        let replacement: Vec<Region> = [
            Region {
                start: region.start,
                end: start,
                allocated: false,
                memory_type: 0,
            },
            Region {
                start,
                end,
                allocated: true,
                memory_type,
            },
            Region {
                start: end,
                end: region.end,
                allocated: false,
                memory_type: 0,
            },
        ]
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect();
        self.regions.splice(index..=index, replacement);
        Some(start)
    }

    /// Takes `len` bytes at a caller-chosen alignment.
    ///
    /// Separate from [`DirectMemory::allocate`] because a stronger alignment changes where a region
    /// can start, not only its size.
    pub fn allocate_aligned(&mut self, len: u64, align: u64, memory_type: u32) -> Option<u64> {
        let align = align.max(DIRECT_ALIGN);
        // Not a power of two is a caller error; rounding would answer a question that was not asked.
        if !align.is_power_of_two() {
            return None;
        }
        // Walks candidate starts rather than adjusting a first fit upward, which could push the end
        // past the region it was chosen from.
        let mut search = 0;
        loop {
            let region = self
                .regions
                .iter()
                .find(|r| !r.allocated && r.end > search && !r.is_empty())?;
            let start = region.start.max(search).checked_next_multiple_of(align)?;
            if start.checked_add(len.checked_next_multiple_of(DIRECT_ALIGN)?)? <= region.end {
                return self.allocate(start, len, memory_type);
            }
            // This region cannot hold it at this alignment; continue past it.
            search = region.end;
        }
    }

    /// Gives back a span, merging it with any free neighbours.
    ///
    /// Without merging, repeated allocate and free fragments the list and every later walk slows.
    pub fn release(&mut self, start: u64, len: u64) -> bool {
        let end = start.saturating_add(len);
        let Some(index) = self
            .regions
            .iter()
            .position(|r| r.allocated && r.start == start && r.end == end)
        else {
            return false;
        };
        self.regions[index].allocated = false;
        self.regions[index].memory_type = 0;
        self.merge_free();
        true
    }

    /// Collapses adjacent free regions into one.
    fn merge_free(&mut self) {
        let mut merged: Vec<Region> = Vec::with_capacity(self.regions.len());
        for region in self.regions.drain(..) {
            match merged.last_mut() {
                Some(previous)
                    if !previous.allocated && !region.allocated && previous.end == region.start =>
                {
                    previous.end = region.end;
                }
                _ => merged.push(region),
            }
        }
        self.regions = merged;
    }

    /// Bytes not yet taken.
    pub fn available(&self) -> u64 {
        self.regions
            .iter()
            .filter(|r| !r.allocated)
            .map(Region::len)
            .sum()
    }

    /// The largest single allocation that could still be placed at `align`.
    ///
    /// Distinct from [`Self::available`], which sums every free byte: free space split across
    /// regions, or an alignment that pushes a start past a region's end, can leave far less
    /// placeable. This lets an out-of-memory answer say whether the pool is full, fragmented or
    /// asked for an unsatisfiable alignment.
    pub fn largest_free_at(&self, align: u64) -> u64 {
        if !align.is_power_of_two() {
            return 0;
        }
        self.regions
            .iter()
            .filter(|r| !r.allocated)
            .filter_map(|r| r.end.checked_sub(r.start.checked_next_multiple_of(align)?))
            .max()
            .unwrap_or(0)
    }
}

/// Renders regions for a diagnostic, listing at most `most` of them.
///
/// A fragmented pool can hold thousands of regions; the elided tail is counted so a truncated
/// list is not mistaken for a short one.
pub fn describe_regions(regions: &[Region], most: usize) -> String {
    let mut out = regions
        .iter()
        .take(most)
        .map(|r| {
            format!(
                "{:#x}..{:#x}{}",
                r.start,
                r.end,
                if r.allocated { " taken" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(rest) = regions.len().checked_sub(most).filter(|n| *n > 0) {
        let _ = write!(out, ", and {rest} more");
    }
    out
}

/// How the physical range is laid out before a guest has touched it.
///
/// A diagnostic setting for the map a guest walks. Shapes other than the default vary the
/// structure the guest sees: how many regions, whether the first starts at zero, and whether
/// there is a gap. A map whose regions do not start at zero, or that has a gap, separates a
/// guest reading the second query field as `end` from one reading it as `size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MapShape {
    /// One free region covering the whole range, from zero.
    Whole,
    /// A reserved block at the bottom, then free memory.
    ///
    /// The hardware does not hand a guest physical zero (see `RESERVED_LOW`).
    ReservedLow,
    /// Alternating taken and free blocks across the range, for a guest looking for a specific
    /// region among several.
    Fragmented,
    /// Free, then a hole nothing describes, then free.
    ///
    /// The only shape that separates the two readings of the second field: after a region ending
    /// at `E` with a gap to `S`, a guest reading `end` queries `E` and one reading `start + size`
    /// queries something else. In every other shape each region begins where the last ended.
    Gapped,
}

impl MapShape {
    /// Every shape, by the name a diagnostic uses.
    ///
    /// The parser and the error message that lists the choices read this one array.
    pub const NAMES: [&str; 4] = ["whole", "reserved-low", "fragmented", "gapped"];

    /// The shape a diagnostic named, or nothing when it named none of them.
    #[must_use]
    pub fn named(text: &str) -> Option<Self> {
        match text {
            "whole" => Some(Self::Whole),
            "reserved-low" => Some(Self::ReservedLow),
            "fragmented" => Some(Self::Fragmented),
            "gapped" => Some(Self::Gapped),
            _ => None,
        }
    }

    /// The regions this shape starts a guest with.
    ///
    /// Sizes are round numbers chosen to be structurally different, not measured figures.
    pub fn regions(self, size: u64) -> Vec<Region> {
        let taken = |start: u64, end: u64| Region {
            start,
            end,
            allocated: true,
            memory_type: 0,
        };
        let free = |start: u64, end: u64| Region {
            start,
            end,
            allocated: false,
            memory_type: 0,
        };
        match self {
            Self::Whole => vec![free(0, size)],
            Self::ReservedLow => {
                let low = RESERVED_LOW.min(size);
                if low == 0 || low >= size {
                    return vec![free(0, size)];
                }
                vec![taken(0, low), free(low, size)]
            }
            // A hole between the two free spans, so the boundary arithmetic differs by the size of the
            // hole and the guest's next query shows which reading it uses.
            Self::Gapped => {
                let block = size / 8;
                if block == 0 || block * 6 >= size {
                    return vec![free(0, size)];
                }
                vec![free(0, block * 2), free(block * 4, size)]
            }
            Self::Fragmented => {
                let low = RESERVED_LOW.min(size);
                let block = size / 8;
                if low == 0 || block == 0 || low + block * 4 >= size {
                    return vec![free(0, size)];
                }
                vec![
                    taken(0, low),
                    free(low, low + block * 2),
                    taken(low + block * 2, low + block * 3),
                    free(low + block * 3, size),
                ]
            }
        }
    }
}

/// How much a shape holds back at the bottom of the range.
///
/// obSCEne's `020-memory/allocate` calls `sceKernelAllocateDirectMemory` with a search start of
/// zero from a clean state and the hardware answers `0x10000`, so the platform reserves the first
/// `0x10000` of the direct range.
const RESERVED_LOW: u64 = 0x1_0000;

/// Memory behaviour that is a choice rather than a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    /// Whether `sceKernelMapNamedDirectMemory` maps. On by default; the switch exists for
    /// bisecting.
    pub map_direct_memory: bool,
    /// What the physical map looks like before a guest touches it.
    ///
    /// Defaults to `ReservedLow`, the layout the hardware shows (`RESERVED_LOW`). The other shapes
    /// are for sweeping the field-meaning and multi-region questions.
    pub map_shape: MapShape,
    /// How large the direct-memory pool is, and what `sceKernelGetDirectMemorySize` reports.
    ///
    /// A setting because it depends on the guest class (see [`DIRECT_MEMORY_SIZE`]). The pool and
    /// the reported size are one field, so they cannot describe different machines.
    pub pool_bytes: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            map_direct_memory: true,
            map_shape: MapShape::ReservedLow,
            pool_bytes: DIRECT_MEMORY_SIZE,
        }
    }
}

/// The settings in force.
fn settings() -> &'static Mutex<Settings> {
    static SETTINGS: OnceLock<Mutex<Settings>> = OnceLock::new();
    SETTINGS.get_or_init(|| Mutex::new(Settings::default()))
}

/// Replaces the memory settings. Called once, during setup.
pub fn configure(new: Settings) {
    if let Ok(mut current) = settings().lock() {
        *current = new;
    }
}

/// The settings in force.
pub fn configured() -> Settings {
    settings().lock().map(|s| *s).unwrap_or_default()
}

/// The one direct-memory map, shared by every guest thread.
///
/// Global because the guest's model is global: one physical range, visible to every thread.
/// A `Mutex`, because allocation is rare next to the work it enables.
pub fn map() -> &'static Mutex<DirectMemory> {
    static MAP: OnceLock<Mutex<DirectMemory>> = OnceLock::new();
    // Built lazily from the settings installed during setup, before any guest call can reach it,
    // so there is one initialisation path.
    MAP.get_or_init(|| {
        let settings = configured();
        Mutex::new(DirectMemory::with_shape(
            settings.pool_bytes,
            settings.map_shape,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{DIRECT_ALIGN, DIRECT_MEMORY_SIZE, DirectMemory, MapShape};

    #[test]
    fn flexible_budget_is_the_measured_pair_and_falls_as_it_is_mapped() {
        // The two queries answer the hardware figures, and available falls by exactly what is mapped
        // and is credited back on release (D444).
        use super::{
            FLEXIBLE_CONFIGURED, FLEXIBLE_MEMORY_SIZE, flexible_available, flexible_configured,
            record_flexible_map, record_flexible_release, reset_flexible,
        };
        reset_flexible();
        assert_eq!(
            flexible_configured(),
            FLEXIBLE_CONFIGURED,
            "configured is the ceiling"
        );
        assert_eq!(
            flexible_available(),
            FLEXIBLE_MEMORY_SIZE,
            "available starts at the launch figure"
        );
        assert!(
            flexible_configured() > flexible_available(),
            "configured is above available, as hardware measured"
        );

        record_flexible_map(0x4000);
        assert_eq!(
            flexible_available(),
            FLEXIBLE_MEMORY_SIZE - 0x4000,
            "map lowers available"
        );
        record_flexible_release(0x4000);
        assert_eq!(
            flexible_available(),
            FLEXIBLE_MEMORY_SIZE,
            "release credits it back"
        );

        // A release that does not match a map cannot hand back more than was taken.
        record_flexible_release(0x10000);
        assert_eq!(
            flexible_available(),
            FLEXIBLE_MEMORY_SIZE,
            "release saturates at the launch figure"
        );
        reset_flexible();
    }

    #[test]
    fn a_fresh_range_is_one_free_region_covering_everything() {
        // A walk must see the whole range accounted for; a gap reads as memory that does not exist.
        let m = DirectMemory::new(1024 * DIRECT_ALIGN);
        assert_eq!(m.regions().len(), 1);
        assert_eq!(m.regions()[0].start, 0);
        assert_eq!(m.regions()[0].end, 1024 * DIRECT_ALIGN);
        assert!(!m.regions()[0].allocated);
        assert_eq!(m.available(), 1024 * DIRECT_ALIGN);
    }

    #[test]
    fn every_shape_still_accounts_for_the_whole_range() {
        // Whatever a shape tests, its regions must tile the range with no gap, since a guest walks by
        // feeding back each region's end.
        use super::MapShape;

        let size = 8 * 1024 * 1024 * 1024;
        for shape in [MapShape::Whole, MapShape::ReservedLow, MapShape::Fragmented] {
            let m = DirectMemory::with_shape(size, shape);
            assert!(!m.regions().is_empty(), "{shape:?} produced nothing");
            assert_eq!(m.regions()[0].start, 0, "{shape:?} does not start at zero");
            assert_eq!(
                m.regions().last().expect("non-empty").end,
                size,
                "{shape:?} does not reach the end"
            );
            for pair in m.regions().windows(2) {
                assert_eq!(
                    pair[0].end, pair[1].start,
                    "{shape:?} leaves a gap at {:#x}",
                    pair[0].end
                );
            }
        }
    }

    #[test]
    fn a_shape_that_will_not_fit_falls_back_rather_than_inventing_a_map() {
        // A range smaller than what a shape holds back is laid out as one free region rather than a
        // truncated or overlapping map.
        use super::MapShape;

        for shape in [MapShape::ReservedLow, MapShape::Fragmented] {
            let m = DirectMemory::with_shape(DIRECT_ALIGN, shape);
            assert_eq!(m.regions().len(), 1, "{shape:?} should fall back");
            assert!(!m.regions()[0].allocated);
        }
    }

    #[test]
    fn a_query_past_the_end_of_a_region_returns_the_next_one() {
        // A guest walks by handing back the end of what it last saw; the query must answer for that
        // offset.
        let mut m = DirectMemory::new(16 * DIRECT_ALIGN);
        m.allocate(0, 4 * DIRECT_ALIGN, 0).expect("allocate");

        let first = m.query(0).expect("something at zero");
        assert!(first.allocated);
        let next = m.query(first.end).expect("something after it");
        assert!(!next.allocated);
        assert_eq!(next.start, first.end);
    }

    #[test]
    fn a_walk_terminates() {
        // A walk must terminate; if it can loop, the guest loops with it.
        let mut m = DirectMemory::new(64 * DIRECT_ALIGN);
        m.allocate(0, DIRECT_ALIGN, 0).expect("allocate");
        m.allocate(8 * DIRECT_ALIGN, DIRECT_ALIGN, 0)
            .expect("allocate");

        let mut offset = 0;
        let mut seen = 0;
        while let Some(region) = m.query(offset) {
            assert!(region.end > offset, "a walk must always advance");
            offset = region.end;
            seen += 1;
            assert!(seen < 100, "the walk did not terminate");
        }
        assert!(seen >= 3);
    }

    #[test]
    fn allocation_splits_a_free_region_and_takes_only_what_was_asked() {
        let mut m = DirectMemory::new(64 * DIRECT_ALIGN);
        let at = m.allocate(0, 4 * DIRECT_ALIGN, 3).expect("allocate");
        assert_eq!(at, 0);
        assert_eq!(m.available(), 60 * DIRECT_ALIGN);

        let taken = m.query(0).expect("the allocation");
        assert!(taken.allocated);
        assert_eq!(taken.len(), 4 * DIRECT_ALIGN);
        assert_eq!(taken.memory_type, 3, "the type is recorded even if unused");
    }

    #[test]
    fn a_request_is_rounded_up_to_the_alignment() {
        // An unaligned span would put the guest's mappings at addresses the address-space layer
        // refuses.
        let mut m = DirectMemory::new(64 * DIRECT_ALIGN);
        m.allocate(0, 1, 0).expect("allocate");
        assert_eq!(m.query(0).expect("region").len(), DIRECT_ALIGN);
    }

    #[test]
    fn a_request_larger_than_anything_free_is_refused_not_partially_met() {
        // A short allocation reported as success lets the guest write past what it was given.
        let mut m = DirectMemory::new(4 * DIRECT_ALIGN);
        assert!(m.allocate(0, 64 * DIRECT_ALIGN, 0).is_none());
        assert_eq!(m.available(), 4 * DIRECT_ALIGN, "nothing was consumed");
    }

    #[test]
    fn releasing_merges_neighbours_so_the_list_does_not_fragment() {
        // Without merging, allocate-and-free in a loop leaves many adjacent free regions.
        let mut m = DirectMemory::new(64 * DIRECT_ALIGN);
        let a = m.allocate(0, DIRECT_ALIGN, 0).expect("a");
        let b = m.allocate(0, DIRECT_ALIGN, 0).expect("b");
        assert!(m.release(a, DIRECT_ALIGN));
        assert!(m.release(b, DIRECT_ALIGN));
        assert_eq!(m.regions().len(), 1, "everything merged back");
        assert_eq!(m.available(), 64 * DIRECT_ALIGN);
    }

    #[test]
    fn releasing_something_that_was_never_allocated_is_refused() {
        // Accepting it would mark free a region a guest is still using.
        let mut m = DirectMemory::new(16 * DIRECT_ALIGN);
        assert!(!m.release(0, DIRECT_ALIGN));
        m.allocate(0, DIRECT_ALIGN, 0).expect("allocate");
        assert!(
            !m.release(0, 2 * DIRECT_ALIGN),
            "a length that does not match must not be honoured"
        );
    }

    /// A region list too long to print says how much it left out.
    #[test]
    fn a_long_region_list_is_cut_short_and_admits_it() {
        let m = DirectMemory::with_shape(DIRECT_MEMORY_SIZE, MapShape::Fragmented);
        let all = super::describe_regions(m.regions(), 64);
        assert!(!all.contains("more"), "nothing elided when they all fit");
        assert_eq!(all.matches("..").count(), m.regions().len());

        let cut = super::describe_regions(m.regions(), 2);
        assert!(cut.ends_with(&format!(", and {} more", m.regions().len() - 2)));
        assert_eq!(cut.matches("..").count(), 2, "only the two it promised");
        assert!(cut.contains("taken"), "allocation is still distinguished");
    }

    /// An alignment nothing can satisfy is distinguishable from a full pool.
    ///
    /// The pool is empty in every case; only the alignment moves.
    #[test]
    fn the_largest_placeable_span_shrinks_as_the_alignment_grows() {
        let m = DirectMemory::with_shape(DIRECT_MEMORY_SIZE, MapShape::ReservedLow);
        assert_eq!(
            m.available(),
            DIRECT_MEMORY_SIZE - super::RESERVED_LOW,
            "every free byte, which is the number that cannot diagnose anything"
        );
        // At the pool's own alignment the whole free span is reachable.
        assert_eq!(
            m.largest_free_at(DIRECT_ALIGN),
            DIRECT_MEMORY_SIZE - super::RESERVED_LOW
        );
        // At half a gibibyte the start is pushed to 0x2000_0000 and that much is lost.
        assert_eq!(
            m.largest_free_at(0x2000_0000),
            DIRECT_MEMORY_SIZE - 0x2000_0000
        );
        // An alignment past the end of the pool leaves nothing placeable at all.
        assert_eq!(m.largest_free_at(DIRECT_MEMORY_SIZE << 1), 0);
        // Not a power of two is not a length question - nothing can be placed at all.
        assert_eq!(m.largest_free_at(3), 0);
    }

    /// A large single direct-memory request a title makes fits the default pool.
    ///
    /// Uses the title's own length rather than a round one, so the alignment arithmetic is the
    /// real one.
    #[test]
    fn the_largest_allocation_a_title_asks_for_fits_the_default_pool() {
        let mut m = DirectMemory::with_shape(DIRECT_MEMORY_SIZE, MapShape::ReservedLow);
        let asked = 0x1_20F0_0000;
        assert!(
            asked < DIRECT_MEMORY_SIZE,
            "the premise: the request is smaller than the pool"
        );
        let address = m.allocate_aligned(asked, DIRECT_ALIGN, 0);
        assert!(
            address.is_some(),
            "a request that fits the pool was refused; free was {:#x}",
            m.available()
        );
    }
}
