//! Direct memory: the physical address space a guest allocates out of.
//!
//! The target exposes its memory as a flat physical range that a guest carves up
//! itself - reserve a span, then map it into the virtual address space separately. It is
//! not `malloc`; it is closer to a physical allocator the application drives.
//!
//! # Why this is the first thing implemented
//!
//! Measurement, not guesswork. Across four commercial executables,
//! `sceKernelDirectMemoryQuery` is **99.9% of every call a guest makes** - one of them
//! called it four hundred million times in ten seconds. That is a guest walking the
//! memory map, being told nothing, and asking again. Nothing else a guest wants matters
//! until it can find out what memory exists.
//!
//! # The model
//!
//! A sorted list of non-overlapping regions covering the whole physical range, each
//! either free or taken. Query walks it; allocation splits a free region; release merges
//! neighbours back. Deliberately simple - the guest does the interesting placement work,
//! and this only has to answer honestly about what it has done so far.
//!
//! # What is deliberately not modelled
//!
//! Memory *types* (the target distinguishes several, with different caching behaviour)
//! are recorded and otherwise ignored. Honouring them means nothing until there is a GPU
//! that cares, and inventing behaviour for them now would be exactly the plausible
//! output principle 3 warns about.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Total direct memory a guest may allocate from.
///
/// **Measured, where it used to be a round figure.** A conformance run on a target console
/// called `sceKernelGetDirectMemorySize` and it answered `0x1_4000_0000` - five gibibytes, not
/// the eight assumed here, and not a power of two, which is why it was never going to be
/// guessed.
///
/// **Verified to matter, before it was known.** A guest reads this and walks the map against
/// it: changing it from 8 GiB to 6 GiB moved the second query a guest made from `0x200000000`
/// to `0x180000000`, exactly tracking. So every title that sizes its heaps off this has been
/// sizing them against a machine three gibibytes larger than the one it was written for
/// (D398).
pub const DIRECT_MEMORY_SIZE: u64 = 0x1_4000_0000;

/// The flexible-memory available figure at launch, before the guest maps any. **Measured, and now
/// applied as a separate budget** (D444).
///
/// obSCEne's `020-memory/flexible-available` answered `0x1b40_0000` on hardware (~437 MiB), against
/// `flexible-configured`'s [`FLEXIBLE_CONFIGURED`] in the same run
/// (`reports/hardware/console-klog.01092026.txt`). Both are the **system default**: every resident title
/// carries an empty `PT_SCE_PROCPARAM` mem-param, the same one obSCEne carries, so none overrides the
/// budget and obSCEne measured it under exactly the condition every title launches under (D442). So both
/// transfer, system-wide like [`DIRECT_MEMORY_SIZE`] and `RESERVED_LOW`.
///
/// Flexible memory is a **separate space** from the direct pool - obSCEne maps it clear of the pool - so
/// it is tracked here as its own budget rather than read off the direct map as it was before (D273): the
/// old reading answered `~5 GiB` where obSCEne under orbistoun expected `0x1b40_0000`, off by an order of
/// magnitude. `available` is this figure minus what the guest has mapped ([`flexible_available`]).
pub const FLEXIBLE_MEMORY_SIZE: u64 = 0x1b40_0000;

/// The configured flexible-memory total - the ceiling the available figure counts down from. **Measured.**
///
/// obSCEne's `020-memory/flexible-configured` answered `0x1c00_0000` on hardware (448 MiB;
/// `reports/hardware/console-klog.01092026.txt`), `0xc0_0000` (12 MiB) above the available figure - the
/// share the system had already mapped when obSCEne asked. System-wide for the same reason as
/// [`FLEXIBLE_MEMORY_SIZE`]: no title overrides it.
pub const FLEXIBLE_CONFIGURED: u64 = 0x1c00_0000;

/// Flexible-memory bytes the guest has mapped, so [`flexible_available`] falls as it should rather than
/// answering a constant a guest that mapped memory would catch lying.
static FLEXIBLE_MAPPED: AtomicU64 = AtomicU64::new(0);

/// The configured flexible-memory total. Constant: the ceiling does not move as memory is mapped.
pub fn flexible_configured() -> u64 {
    FLEXIBLE_CONFIGURED
}

/// The flexible memory available to map now - the launch figure minus what the guest has mapped.
pub fn flexible_available() -> u64 {
    FLEXIBLE_MEMORY_SIZE.saturating_sub(FLEXIBLE_MAPPED.load(Ordering::Relaxed))
}

/// Records a flexible mapping of `len` bytes against the budget.
pub fn record_flexible_map(len: u64) {
    FLEXIBLE_MAPPED.fetch_add(len, Ordering::Relaxed);
}

/// Returns `len` bytes to the flexible budget on release. Saturating, so a release that does not match a
/// map cannot drive the counter below zero and hand back more than was ever taken.
pub fn record_flexible_release(len: u64) {
    let _ = FLEXIBLE_MAPPED.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |mapped| {
        Some(mapped.saturating_sub(len))
    });
}

/// Resets the flexible-memory budget. For tests, so one does not see another's mappings.
#[cfg(test)]
pub fn reset_flexible() {
    FLEXIBLE_MAPPED.store(0, Ordering::Relaxed);
}

/// Alignment every direct-memory boundary satisfies.
///
/// The vendor's direct memory is handed out in 16 KiB units, which is also what
/// [`orbistoun_core::DIRECT_MEMORY_ALIGN`] records for the address-space layer.
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
    /// The memory type the guest asked for, recorded and not yet acted on.
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
    /// Separate from [`Self::new`] so the default stays the thing every existing
    /// measurement was taken against, and a different shape is something somebody asked
    /// for (D218).
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
    /// **This is what a guest enumerating memory actually asks.** It walks by handing
    /// back the end of what it last saw, so a query that only answered for offsets
    /// inside a region would stall the walk at the first gap - and a guest that cannot
    /// finish a walk repeats it, which is exactly the four-hundred-million-call loop
    /// this set out to fix.
    pub fn query(&self, offset: u64) -> Option<Region> {
        self.regions
            .iter()
            .find(|r| r.end > offset && !r.is_empty())
            .copied()
    }

    /// Takes `len` bytes, searching from `search_start`.
    ///
    /// Returns the physical address, or `None` when nothing large enough is free. First
    /// fit: the guest decides placement policy, and a cleverer strategy here would only
    /// disagree with it.
    pub fn allocate(&mut self, search_start: u64, len: u64, memory_type: u32) -> Option<u64> {
        // Checked, not panicking: this is reachable from a guest call, and an unwind
        // across that boundary is undefined behaviour rather than a panic message (D156).
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

        // Replace the free region with up to three: the untouched head, the new
        // allocation, and the untouched tail. Empty ones are dropped rather than kept,
        // because a zero-length region in the list would be walked past forever.
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
    /// Separate from [`DirectMemory::allocate`] because an alignment stronger than the
    /// pool's own changes where a region can start, not just how big it is - and a caller
    /// asking for one is asking because its hardware requires it. Silently ignoring that
    /// hands back an address that works everywhere except where it matters.
    pub fn allocate_aligned(&mut self, len: u64, align: u64, memory_type: u32) -> Option<u64> {
        let align = align.max(DIRECT_ALIGN);
        // Not a power of two is a caller error, not something to round into shape:
        // rounding would answer a question that was not asked.
        if !align.is_power_of_two() {
            return None;
        }
        // Searched by walking candidate starts rather than by asking for the first fit
        // and adjusting: adjusting upward can push the end past the region it was chosen
        // from, which is how an allocator hands out memory belonging to something else.
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
    /// Merging matters: without it a guest that allocates and frees repeatedly leaves
    /// the list fragmented into thousands of adjacent free regions, and every subsequent
    /// walk gets slower for no reason a reader could see.
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
}

/// How the physical range is laid out before a guest has touched it.
///
/// # The one variable a guest reacts to that has never been varied
///
/// PPSA04263 spends **99.9% of every call it makes** walking the memory map and rejecting
/// what it sees - 852 million queries in twenty seconds, and it never reaches
/// `sceKernelAllocateDirectMemory` at all. Three things about the answer have been swept
/// and changed nothing: the return code (ten candidates, both signs), the third structure
/// field (0..10), and whether the buffer is cleared at the end of the walk. One thing has
/// never been swept: **the map has always been a single free region starting at zero**
/// (D083).
///
/// That is not just an untried option, it is the reason the third-field sweep proves less
/// than it appears to. A guest hunting for *a region matching some criterion* cannot
/// distinguish "wrong value" from "wrong shape" when there is only ever one region to
/// look at - the same underpowered-experiment shape as D187, where an `ok` sweep reported
/// no change because the functions under test never saw it.
///
/// # And it makes the structure layout falsifiable for the first time
///
/// The second field is written as `end`. If the real structure carries `size` instead, a
/// map of one region starting at zero writes **the same number either way** - so the
/// current model cannot tell those layouts apart, and neither can any experiment run
/// against it. Any map whose first region does not start at zero separates them
/// immediately (D218).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MapShape {
    /// One free region covering the whole range, from zero.
    ///
    /// The simplest thing that could work, and what every measurement so far was taken
    /// against.
    Whole,
    /// A reserved block at the bottom, then free memory.
    ///
    /// Real hardware does not hand a guest physical zero. If the guest is skipping or
    /// rejecting a region at offset zero, this is what shows it - and because the free
    /// region no longer starts at zero, `end` and `size` stop being the same number.
    ReservedLow,
    /// Alternating taken and free blocks across the range.
    ///
    /// What a walk is actually *for*. If the guest wants somewhere specific among several
    /// candidates, a map with one entry can never satisfy it however the entry is
    /// labelled.
    Fragmented,
    /// Free, then a hole nothing describes, then free.
    ///
    /// **The only shape that can settle what the second field means.** In every other shape
    /// each region begins exactly where the last ended, so a guest feeding back the previous
    /// end and one feeding back `start + size` produce identical offsets - the two readings
    /// are indistinguishable by construction, which is what D218 recorded as still open.
    ///
    /// A hole separates them: after a region ending at `E` with a gap to `S`, a guest reading
    /// `end` queries `E` and one reading `start + size` queries something else. `Fragmented`
    /// was believed to be this experiment and is not - it has more regions and no gap (D357).
    Gapped,
}

impl MapShape {
    /// Every shape, by the name a diagnostic uses.
    ///
    /// **One list, because two would drift.** The parser and the error message that lists the
    /// choices read the same array, so a shape added to the enum and forgotten here is a
    /// compile error rather than a value nothing accepts (D356).
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
    /// Sizes are round numbers rather than measured ones, and that is the honest state:
    /// nothing has established what the target's map looks like. They are chosen only to
    /// be *structurally* different from each other, because what is being tested is the
    /// shape, not the figures.
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
            // A hole between the two free spans, so the boundary arithmetic differs by the
            // size of the hole and the guest's next query says which reading it uses.
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
/// **Measured.** obSCEne's `020-memory/allocate` on real hardware calls
/// `sceKernelAllocateDirectMemory` with a search start of zero from a clean state and is answered
/// `0x10000` - the first free physical offset, so the platform reserves the first `0x10000` of the
/// direct range and never hands a guest physical zero (obSCEne `reports/hardware/console-report-klog.txt`
/// and `ps5-full-run.txt`; a later allocation in the same run was answered `0xff0000`, above this
/// floor). This was an arbitrary 512 MiB until that measurement retired the guess (D083, D218).
const RESERVED_LOW: u64 = 0x1_0000;

/// Memory behaviour that is a choice rather than a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    /// Whether `sceKernelMapNamedDirectMemory` actually maps.
    ///
    /// **On, and the history is worth keeping.** It was off for one afternoon because
    /// enabling it took PPSA28061 from 38 imports to 15 and moved the fault into host
    /// code. That was not a bug in the mapping: guest calls were arriving on a misaligned
    /// stack, every earlier import was small enough not to notice, and this was simply
    /// the first one doing enough work for the compiler to use an instruction that cares
    /// (D159).
    ///
    /// With the entry convention corrected it is worth +8 imports and +427 calls, and the
    /// switch stays because turning a subsystem off is a useful thing to be able to do
    /// when bisecting.
    pub map_direct_memory: bool,
    /// What the physical map looks like before a guest touches it.
    ///
    /// **Defaults to `ReservedLow` since 2026-09-01, because hardware measured it.** It was `Whole`
    /// (one free region from zero) for as long as nothing had measured otherwise - the deliberately
    /// conservative default D218 argued for. obSCEne's `020-memory/allocate` then answered `0x10000`
    /// from a clean state, which is exactly the "the map does not start at zero" that `ReservedLow`
    /// models, so that is now the default and `RESERVED_LOW` is the measured floor. The other
    /// shapes remain for sweeping the questions the walk still leaves open (the `end`-vs-`size` field
    /// meaning; the multi-region case).
    pub map_shape: MapShape,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            map_direct_memory: true,
            map_shape: MapShape::ReservedLow,
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

/// The settings in force right now.
pub fn configured() -> Settings {
    settings().lock().map(|s| *s).unwrap_or_default()
}

/// The one direct-memory map, shared by every guest thread.
///
/// Global because the guest's own model is global: there is one physical range, and a
/// thread that allocates from it must be visible to every other. A `Mutex` rather than
/// anything cleverer because allocation is rare next to the work it enables, and the
/// simplest correct thing is the right starting point.
pub fn map() -> &'static Mutex<DirectMemory> {
    static MAP: OnceLock<Mutex<DirectMemory>> = OnceLock::new();
    // Built from the settings in force, which are installed during setup and therefore
    // before any guest call can reach this. Reading them here rather than at
    // `configure` time keeps one initialisation path: a map built eagerly at startup and
    // then reconfigured would be two, and the second would be the one nothing tested.
    MAP.get_or_init(|| {
        Mutex::new(DirectMemory::with_shape(
            DIRECT_MEMORY_SIZE,
            configured().map_shape,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{DIRECT_ALIGN, DirectMemory};

    #[test]
    fn flexible_budget_is_the_measured_pair_and_falls_as_it_is_mapped() {
        // The property that matters: the two queries answer the measured hardware figures, and
        // available falls by exactly what is mapped and is credited back on release - not a
        // constant a guest that maps then re-queries would catch lying (D444).
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
        // A guest walking memory must see the whole range accounted for. A gap would
        // read as memory that does not exist.
        let m = DirectMemory::new(1024 * DIRECT_ALIGN);
        assert_eq!(m.regions().len(), 1);
        assert_eq!(m.regions()[0].start, 0);
        assert_eq!(m.regions()[0].end, 1024 * DIRECT_ALIGN);
        assert!(!m.regions()[0].allocated);
        assert_eq!(m.available(), 1024 * DIRECT_ALIGN);
    }

    #[test]
    fn every_shape_still_accounts_for_the_whole_range() {
        // **The property that matters more than the shape.** A guest enumerates by feeding
        // back the end of each region it is shown, so a gap between two regions reads as
        // memory that does not exist and a walk that steps over it is a walk of a machine
        // nobody has. Whatever a shape is testing, it must not accidentally test that.
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
        // A range smaller than what a shape holds back cannot be laid out that way. One
        // free region is the honest answer; a truncated or overlapping map would be a
        // machine that does not exist, described confidently.
        use super::MapShape;

        for shape in [MapShape::ReservedLow, MapShape::Fragmented] {
            let m = DirectMemory::with_shape(DIRECT_ALIGN, shape);
            assert_eq!(m.regions().len(), 1, "{shape:?} should fall back");
            assert!(!m.regions()[0].allocated);
        }
    }

    #[test]
    fn a_query_past_the_end_of_a_region_returns_the_next_one() {
        // How a guest walks: it hands back the end of what it last saw. Answering only
        // for offsets *inside* a region stalls the walk, and a guest that cannot finish
        // a walk starts it again - which is the loop this exists to break.
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
        // The property that matters most. If a walk can loop, the guest loops with it.
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
        // Handing back an unaligned span would put the guest's own mappings at addresses
        // the address-space layer refuses.
        let mut m = DirectMemory::new(64 * DIRECT_ALIGN);
        m.allocate(0, 1, 0).expect("allocate");
        assert_eq!(m.query(0).expect("region").len(), DIRECT_ALIGN);
    }

    #[test]
    fn a_request_larger_than_anything_free_is_refused_not_partially_met() {
        // A short allocation reported as success is the worst possible answer: the guest
        // writes past the end of what it was given.
        let mut m = DirectMemory::new(4 * DIRECT_ALIGN);
        assert!(m.allocate(0, 64 * DIRECT_ALIGN, 0).is_none());
        assert_eq!(m.available(), 4 * DIRECT_ALIGN, "nothing was consumed");
    }

    #[test]
    fn releasing_merges_neighbours_so_the_list_does_not_fragment() {
        // Without merging, allocate-and-free in a loop leaves thousands of adjacent free
        // regions and every later walk gets slower for no visible reason.
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
        // Silently accepting it would mark a region free that a guest is still using.
        let mut m = DirectMemory::new(16 * DIRECT_ALIGN);
        assert!(!m.release(0, DIRECT_ALIGN));
        m.allocate(0, DIRECT_ALIGN, 0).expect("allocate");
        assert!(
            !m.release(0, 2 * DIRECT_ALIGN),
            "a length that does not match must not be honoured"
        );
    }
}
