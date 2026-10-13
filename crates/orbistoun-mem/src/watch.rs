//! Whether guest memory has been written since a point.
//!
//! A copy of guest memory (a colour target written back at a flip, a texture's texels) has to
//! know whether the guest changed it since. Guest memory is reserved write-watched, and the host
//! records which pages anything in the process wrote. [`mark`] says "now" for a range and
//! [`written_since`] says whether any of its pages were written after a mark; each harvest is
//! kept per page as its epoch, so several askers share one host record. `None` means the host
//! cannot say (the range is not watched, or the host is not Windows), and the asker compares
//! bytes instead.

use std::collections::BTreeMap;
use std::sync::Mutex;

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
}

fn record() -> &'static Mutex<Record> {
    static RECORD: Mutex<Record> = Mutex::new(Record {
        epoch: 1,
        written: BTreeMap::new(),
        chunks: BTreeMap::new(),
    });
    &RECORD
}

impl Record {
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
    record.harvest(base, len)?;
    record.epoch += 1;
    Some(epoch)
}

/// Whether any page of `[base, base + len)` was written after `since`, a [`mark`] of it. `None`
/// when the host cannot say.
pub fn written_since(base: u64, len: u64, since: u64) -> Option<bool> {
    let mut record = record()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A page found written now was written after any earlier mark: the epoch has moved past
    // every one handed out.
    if record.harvest(base, len)? {
        return Some(true);
    }
    let first = base & !(PAGE - 1);
    Some(record.written_after((first, base.saturating_add(len)), since))
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Memory::{
        GetWriteWatch, MEMORY_BASIC_INFORMATION, VirtualQuery,
    };

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

    /// A write after a mark is seen, a write before it is not, and two askers do not disturb each
    /// other.
    #[test]
    fn writes_after_a_mark_are_seen_and_before_it_are_not() {
        #[allow(
            non_snake_case,
            reason = "a fixed address, named as the constants it stands for are"
        )]
        let WATCH_BASE = crate::unique_test_base();
        let len = 0x10000;
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
