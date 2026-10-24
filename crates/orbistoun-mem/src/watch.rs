//! Whether guest memory has been written since a point.
//!
//! A copy of guest memory (a colour target written back at a flip, a texture's texels) has to
//! know whether the guest changed it since. Guest memory is reserved write-watched, and the host
//! records which pages anything in the process wrote. [`mark`] says "now" for a range and
//! [`written_since`] says whether any of its pages were written after a mark; each harvest is
//! kept per page as its epoch, so several askers share one host record. `None` means the host
//! cannot say (the range is not watched, or the host is not Windows), and the asker compares
//! bytes instead.
//!
//! Asking the host walks every page of the range in its kernel, which for a 34 MB texture took
//! 0.46 ms a binding. So a large range a mark is taken of is also made read-only (D824): while it
//! stays so, nothing has written it, and the answer needs no call. The first write faults, the
//! host's fault handler hands it to [`write_faulted`], the range is made writable again and the
//! write runs, seen from then on as any write is. Anything else that changes those pages - the
//! guest's own protection or unmapping, another guard, or the host writing them from its kernel,
//! as a file read does - releases them first through [`release_overlapping`].

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The host page write-watch reports in.
const PAGE: u64 = 4096;

/// The span [`Record::chunks`] keeps one latest epoch for.
const CHUNK: u64 = 1 << 20;

/// Each page ever harvested as written, to the epoch it was harvested in; each chunk of pages, to
/// the latest of theirs; and the current epoch.
///
/// The chunks keep a question about a large range from reading every page it holds: PPSA25872's
/// textures span 34 MB, some 8 000 pages of it written once at load, and reading them all at every
/// binding cost 0.1 ms of each.
struct Record {
    epoch: u64,
    written: BTreeMap<u64, u64>,
    chunks: BTreeMap<u64, u64>,
    /// The ranges read-only since a mark, none overlapping another (D824).
    guards: Vec<Guard>,
}

/// Whole pages a [`mark`] made read-only: nothing has written them since.
#[derive(Debug, Clone, Copy)]
struct Guard {
    first: u64,
    end: u64,
}

fn record() -> &'static Mutex<Record> {
    static RECORD: Mutex<Record> = Mutex::new(Record {
        epoch: 1,
        written: BTreeMap::new(),
        chunks: BTreeMap::new(),
        guards: Vec::new(),
    });
    &RECORD
}

/// Whether marks may guard: set once a fault handler that asks [`write_faulted`] is installed, and
/// never before, since a guarded page's first write would otherwise end the process.
static GUARDING: AtomicBool = AtomicBool::new(false);

/// How many guards stand, so a release with none to find takes no lock.
static GUARDS: AtomicUsize = AtomicUsize::new(0);

/// The shortest range a mark guards: below it, asking the host costs little. Asking costs some
/// microseconds a range whatever its size - Craft's 16 KiB to 1 MiB chunk buffers, asked of every
/// draw, spent 13% of its run there - so a range of sixteen pages or more is guarded.
const GUARDED_FROM: u64 = 64 << 10;

/// The most guards that stand at once: a title asks of hundreds of buffers.
const GUARDS_MOST: usize = 1024;

/// Lets [`mark`] guard large ranges from now on. Called by whoever installs the fault handler that
/// asks [`write_faulted`], once it is installed.
pub fn guard_marked_ranges() {
    GUARDING.store(true, Ordering::Release);
}

/// The whole pages `[base, base + len)` lies in, as `(first, end)`.
fn pages_of(base: u64, len: u64) -> (u64, u64) {
    (
        base & !(PAGE - 1),
        base.saturating_add(len).saturating_add(PAGE - 1) & !(PAGE - 1),
    )
}

impl Record {
    /// The guard holding every page of `(first, end)`, if one does.
    fn guard_over(&self, (first, end): (u64, u64)) -> Option<Guard> {
        self.guards
            .iter()
            .copied()
            .find(|guard| guard.first <= first && end <= guard.end)
    }

    /// Makes `(first, end)` writable again and forgets its guard, noting every page written now:
    /// whatever released it may have changed any of them.
    fn release(&mut self, at: usize) {
        let guard = self.guards.swap_remove(at);
        GUARDS.fetch_sub(1, Ordering::Release);
        imp::unguard(guard.first, guard.end);
        let epoch = self.epoch;
        for page in (guard.first..guard.end).step_by(PAGE as usize) {
            self.note(page, epoch);
        }
    }

    /// Records `page` as harvested written at `epoch`.
    fn note(&mut self, page: u64, epoch: u64) {
        self.written.insert(page, epoch);
        let latest = self.chunks.entry(page & !(CHUNK - 1)).or_insert(0);
        *latest = (*latest).max(epoch);
    }

    /// Moves every page of `[base, base + len)` the host has seen written since its last harvest
    /// into the record at the current epoch. Answers whether it found any; `None` when part of the
    /// range is not watched.
    fn harvest(&mut self, base: u64, len: u64) -> Option<bool> {
        let epoch = self.epoch;
        let mut found = false;
        imp::harvest(base, len, &mut |page| {
            self.note(page, epoch);
            found = true;
        })?;
        Some(found)
    }

    /// Whether any page whose start lies in `[first, end)` was harvested after `since`: a chunk
    /// wholly inside the range answers from its latest epoch, and only a chunk the range cuts reads
    /// its pages.
    fn written_after(&self, (first, end): (u64, u64), since: u64) -> bool {
        self.chunks
            .range(first & !(CHUNK - 1)..end)
            .filter(|&(_, &latest)| latest > since)
            .any(|(&chunk, _)| {
                let (from, to) = (chunk.max(first), chunk.saturating_add(CHUNK).min(end));
                (from == chunk && to == chunk.saturating_add(CHUNK))
                    || self
                        .written
                        .range(from..to)
                        .any(|(_, &harvested)| harvested > since)
            })
    }
}

/// A point in time for `[base, base + len)`: every write to it before now is behind the answer,
/// every write after it will be seen by [`written_since`]. `None` when the host cannot watch it.
pub fn mark(base: u64, len: u64) -> Option<u64> {
    let mut record = record()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let epoch = record.epoch;
    let (first, end) = pages_of(base, len);
    // Guarded before the harvest, so a write landing between the two is in the harvest, behind
    // this mark, rather than in neither.
    let guarded = GUARDING.load(Ordering::Acquire)
        && len >= GUARDED_FROM
        && record.guards.len() < GUARDS_MOST
        && !record
            .guards
            .iter()
            .any(|guard| guard.first < end && first < guard.end)
        && imp::guard(first, end);
    if record.harvest(base, len).is_none() {
        if guarded {
            imp::unguard(first, end);
        }
        return None;
    }
    if guarded {
        record.guards.push(Guard { first, end });
        GUARDS.fetch_add(1, Ordering::Release);
    }
    record.epoch += 1;
    Some(epoch)
}

/// Whether any page of `[base, base + len)` was written after `since`, a [`mark`] of it. `None`
/// when the host cannot say.
pub fn written_since(base: u64, len: u64, since: u64) -> Option<bool> {
    let mut record = record()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Under a guard nothing has written it since the guard's mark, and the host has nothing to
    // hand over that the mark's harvest did not take: the record answers alone.
    let (first, end) = pages_of(base, len);
    if record.guard_over((first, end)).is_some() {
        return Some(record.written_after((first, base.saturating_add(len)), since));
    }
    // A page found written now was written after any earlier mark: the epoch has moved past
    // every one handed out.
    if record.harvest(base, len)? {
        return Some(true);
    }
    let first = base & !(PAGE - 1);
    Some(record.written_after((first, base.saturating_add(len)), since))
}

/// A write faulted at `address`: when a guard holds it, its pages are made writable again and
/// noted written, and the write can run again. The host's fault handler asks this for every
/// faulting write, before anything else that guards pages.
///
/// It waits for the record: no holder of it writes guest memory, so the thread that faulted is
/// never the one holding it.
pub fn write_faulted(address: u64) -> bool {
    if GUARDS.load(Ordering::Acquire) == 0 {
        return false;
    }
    let mut record = record()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(at) = record
        .guards
        .iter()
        .position(|guard| guard.first <= address && address < guard.end)
    else {
        return false;
    };
    record.release(at);
    true
}

/// Before anything but a guest instruction changes `[base, base + len)` - the host writing it from
/// its kernel, another guard: its pages not yet committed are committed, since a write from the
/// host's kernel reaches no fault handler ([`crate::lazy`]), and every guard on them is released
/// and its pages noted written.
pub fn release_overlapping(base: u64, len: u64) {
    crate::lazy::commit(base, len);
    release_guards(base, len);
}

/// [`release_overlapping`] without the commit: before a change of protection or mapping, which
/// commits nothing.
pub(crate) fn release_guards(base: u64, len: u64) {
    if GUARDS.load(Ordering::Acquire) == 0 {
        return;
    }
    let (first, end) = pages_of(base, len);
    let mut record = record()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while let Some(at) = record
        .guards
        .iter()
        .position(|guard| guard.first < end && first < guard.end)
    {
        record.release(at);
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Memory::{
        GetWriteWatch, MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_READONLY, PAGE_READWRITE,
        VirtualProtect, VirtualQuery,
    };

    /// What the host says of the pages from `at`: where their run of alike pages ends, its state
    /// and protection. `None` when it says nothing.
    fn query(at: usize) -> Option<(usize, u32, u32)> {
        let mut info = MEMORY_BASIC_INFORMATION {
            BaseAddress: std::ptr::null_mut(),
            AllocationBase: std::ptr::null_mut(),
            AllocationProtect: 0,
            PartitionId: 0,
            RegionSize: 0,
            State: 0,
            Protect: 0,
            Type: 0,
        };
        // SAFETY: querying any address is safe; the structure is the size passed and outlives the
        // call.
        let answered = unsafe {
            VirtualQuery(
                std::ptr::with_exposed_provenance::<core::ffi::c_void>(at),
                &raw mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        let run_end = (info.BaseAddress as usize).saturating_add(info.RegionSize);
        (answered != 0 && run_end > at).then_some((run_end, info.State, info.Protect))
    }

    /// Sets `protection` on `[first, end)`, a piece at a time as the host's regions lie.
    fn set(first: usize, end: usize, protection: u32) -> bool {
        let mut at = first;
        while at < end {
            let Some((run_end, _, _)) = query(at) else {
                return false;
            };
            let piece_end = run_end.min(end);
            let mut old = 0u32;
            // SAFETY: `[at, piece_end)` is one run of committed guest pages the caller found plain
            // read-write; changing their protection changes no bytes.
            let ok = unsafe {
                VirtualProtect(
                    std::ptr::with_exposed_provenance_mut::<core::ffi::c_void>(at),
                    piece_end - at,
                    protection,
                    &raw mut old,
                )
            };
            if ok == 0 {
                return false;
            }
            at = piece_end;
        }
        true
    }

    /// Makes `[first, end)` read-only, when every page of it is committed and plain read-write -
    /// anything else (another guard's pages among them) is left alone. Whether it did.
    pub(super) fn guard(first: u64, end: u64) -> bool {
        let (Ok(first), Ok(end)) = (usize::try_from(first), usize::try_from(end)) else {
            return false;
        };
        let mut at = first;
        while at < end {
            match query(at) {
                Some((run_end, MEM_COMMIT, PAGE_READWRITE)) => at = run_end,
                _ => return false,
            }
        }
        if set(first, end, PAGE_READONLY) {
            return true;
        }
        set(first, end, PAGE_READWRITE);
        false
    }

    /// Makes `[first, end)` read-write again, as far as it is still mapped.
    pub(super) fn unguard(first: u64, end: u64) {
        if let (Ok(first), Ok(end)) = (usize::try_from(first), usize::try_from(end)) {
            set(first, end, PAGE_READWRITE);
        }
    }

    /// `WRITE_WATCH_FLAG_RESET`, `1` - declared under `Win32_System_SystemServices`
    /// (`SystemServices/mod.rs:5047` in 0.61.2), as `MEM_WRITE_WATCH` is.
    const WRITE_WATCH_FLAG_RESET: u32 = 1;

    /// Addresses one call hands back at most.
    const BATCH: usize = 512;

    /// Hands `found` every page of `[base, base + len)` the host has seen written since its last
    /// harvest, resetting the host's record of them. `None`, having handed over what it could,
    /// when part of the range is not write-watched.
    pub(super) fn harvest(base: u64, len: u64, found: &mut impl FnMut(u64)) -> Option<()> {
        let mut at = usize::try_from(base).ok()?;
        let end = at.checked_add(usize::try_from(len).ok()?)?;
        while at < end {
            let mut info = MEMORY_BASIC_INFORMATION {
                BaseAddress: std::ptr::null_mut(),
                AllocationBase: std::ptr::null_mut(),
                AllocationProtect: 0,
                PartitionId: 0,
                RegionSize: 0,
                State: 0,
                Protect: 0,
                Type: 0,
            };
            // SAFETY: querying any address is safe; the structure is the size passed and outlives
            // the call.
            let answered = unsafe {
                VirtualQuery(
                    std::ptr::with_exposed_provenance::<core::ffi::c_void>(at),
                    &raw mut info,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            // One query answers for a run of pages alike, which lies inside one allocation - the
            // unit the host's watch is kept per.
            let run_end = (info.BaseAddress as usize).saturating_add(info.RegionSize);
            if answered == 0 || run_end <= at {
                return None;
            }
            let segment_end = run_end.min(end);
            loop {
                let mut addresses = [std::ptr::null_mut::<core::ffi::c_void>(); BATCH];
                let mut count = BATCH;
                let mut granularity = 0u32;
                // SAFETY: `[at, segment_end)` is part of one allocation (the query above); the
                // array holds `count` entries, and every out-parameter outlives the call.
                let failed = unsafe {
                    GetWriteWatch(
                        WRITE_WATCH_FLAG_RESET,
                        std::ptr::with_exposed_provenance::<core::ffi::c_void>(at),
                        segment_end - at,
                        addresses.as_mut_ptr(),
                        &raw mut count,
                        &raw mut granularity,
                    )
                };
                if failed != 0 {
                    return None;
                }
                for page in &addresses[..count.min(BATCH)] {
                    found(*page as u64);
                }
                if count < BATCH {
                    break;
                }
            }
            at = segment_end;
        }
        Some(())
    }
}

#[cfg(not(windows))]
mod imp {
    /// No watch away from Windows yet: every asker compares the bytes.
    pub(super) fn harvest(_base: u64, _len: u64, _found: &mut impl FnMut(u64)) -> Option<()> {
        None
    }

    /// Nothing is guarded where nothing is watched.
    pub(super) fn guard(_first: u64, _end: u64) -> bool {
        false
    }

    /// Nothing was guarded.
    pub(super) fn unguard(_first: u64, _end: u64) {}
}

#[cfg(test)]
mod record_tests {
    use super::{CHUNK, PAGE, Record};
    use std::collections::BTreeMap;

    /// The chunked answer is the page-by-page one: for ranges inside a chunk, across chunk edges,
    /// covering chunks whole, and before or after every write.
    #[test]
    fn written_after_answers_as_reading_every_page() {
        let mut record = Record {
            epoch: 1,
            written: BTreeMap::new(),
            chunks: BTreeMap::new(),
            guards: Vec::new(),
        };
        let pages = [
            (0, 2),
            (PAGE, 5),
            (CHUNK - PAGE, 3),
            (CHUNK, 7),
            (3 * CHUNK + 5 * PAGE, 4),
        ];
        for (page, epoch) in pages {
            record.note(page, epoch);
        }
        let naive = |(first, end): (u64, u64), since: u64| {
            pages
                .iter()
                .any(|&(page, epoch)| page >= first && page < end && epoch > since)
        };
        let edges = [
            0,
            PAGE,
            2 * PAGE,
            CHUNK - PAGE,
            CHUNK,
            CHUNK + PAGE,
            2 * CHUNK,
            3 * CHUNK + 5 * PAGE,
            3 * CHUNK + 6 * PAGE,
            5 * CHUNK,
        ];
        for &first in &edges {
            for &end in edges.iter().filter(|&&end| end > first) {
                for since in 0..8 {
                    assert_eq!(
                        record.written_after((first, end), since),
                        naive((first, end), since),
                        "[{first:#x}, {end:#x}) since {since}"
                    );
                }
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{mark, written_since};
    use crate::Protection;
    use crate::platform::reserve;

    /// The protection the host reports for the page at `address`.
    fn protection_at(address: u64) -> u32 {
        use windows_sys::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};
        // SAFETY: an all-zero `MEMORY_BASIC_INFORMATION` is a valid value; the query overwrites
        // it.
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
        // SAFETY: querying any address is safe; the structure is the size passed and outlives the
        // call.
        unsafe {
            VirtualQuery(
                std::ptr::with_exposed_provenance::<core::ffi::c_void>(address as usize),
                &raw mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        info.Protect
    }

    /// A large range marked is read-only, and answered unwritten with nothing asked of the host; a
    /// fault on it, or a release of it, makes it writable again and answers it written. A small
    /// range is never guarded. No write lands on a guarded page here: a test has no handler to
    /// answer its fault, so the handler's half is called as it would call it.
    #[test]
    fn a_large_marked_range_is_guarded_until_released() {
        use windows_sys::Win32::System::Memory::{PAGE_READONLY, PAGE_READWRITE};
        let base = crate::unique_test_base();
        let len = 2 * super::GUARDED_FROM;
        let _held = reserve(base, len, Protection::READ_WRITE).expect("reserve");
        super::guard_marked_ranges();

        let since = mark(base, len).expect("watched");
        assert_eq!(protection_at(base + len - 1), PAGE_READONLY, "guarded");
        assert_eq!(written_since(base, len, since), Some(false));
        assert!(super::write_faulted(base + 0x5000), "the guard answers");
        assert_eq!(protection_at(base), PAGE_READWRITE, "writable again");
        assert!(!super::write_faulted(base + 0x5000), "and is gone");
        assert_eq!(written_since(base, len, since), Some(true), "noted written");

        let again = mark(base, len).expect("watched");
        assert_eq!(protection_at(base), PAGE_READONLY);
        super::release_overlapping(base + len - 1, 1);
        assert_eq!(protection_at(base), PAGE_READWRITE, "released by a touch");
        assert_eq!(written_since(base, len, again), Some(true));

        let small = mark(base, 0x1000).expect("watched");
        assert_eq!(protection_at(base), PAGE_READWRITE, "too small to guard");
        assert_eq!(written_since(base, 0x1000, small), Some(false));
    }

    /// A write after a mark is seen, a write before it is not, and two askers do not disturb each
    /// other.
    #[test]
    fn writes_after_a_mark_are_seen_and_before_it_are_not() {
        #[allow(
            non_snake_case,
            reason = "a fixed address, named as the constants it stands for are"
        )]
        let WATCH_BASE = crate::unique_test_base();
        // Below the guarded size: another test turns guarding on for the process, and a guarded
        // range's write faults into a handler only a worker installs.
        let len = super::GUARDED_FROM / 2;
        let _held = reserve(WATCH_BASE, len, Protection::READ_WRITE).expect("reserve");
        let bytes = std::ptr::with_exposed_provenance_mut::<u8>(WATCH_BASE as usize);
        // SAFETY: the reservation above is `len` read-write bytes at this address, held for the
        // test.
        unsafe { bytes.write(1) };
        let first = mark(WATCH_BASE, len).expect("watched");
        assert_eq!(
            written_since(WATCH_BASE, len, first),
            Some(false),
            "before the mark"
        );
        let second = mark(WATCH_BASE, 0x1000).expect("watched");
        let later = bytes.wrapping_add(0x2000);
        // SAFETY: as above, two pages further in and still inside the reservation.
        unsafe { later.write(2) };
        assert_eq!(
            written_since(WATCH_BASE, len, first),
            Some(true),
            "after the first mark"
        );
        assert_eq!(
            written_since(WATCH_BASE, 0x1000, second),
            Some(false),
            "a page nobody wrote"
        );
        assert_eq!(
            written_since(WATCH_BASE, len, first),
            Some(true),
            "still seen by an asker whose harvest another took"
        );
    }
}
