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

/// Each page ever harvested as written, to the epoch it was harvested in; and the current epoch.
struct Record {
    epoch: u64,
    written: BTreeMap<u64, u64>,
}

fn record() -> &'static Mutex<Record> {
    static RECORD: Mutex<Record> = Mutex::new(Record {
        epoch: 1,
        written: BTreeMap::new(),
    });
    &RECORD
}

/// A point in time for `[base, base + len)`: every write to it before now is behind the answer,
/// every write after it will be seen by [`written_since`]. `None` when the host cannot watch it.
pub fn mark(base: u64, len: u64) -> Option<u64> {
    let mut record = record()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let epoch = record.epoch;
    imp::harvest(base, len, epoch, &mut record.written)?;
    record.epoch += 1;
    Some(epoch)
}

/// Whether any page of `[base, base + len)` was written after `since`, a [`mark`] of it. `None`
/// when the host cannot say.
pub fn written_since(base: u64, len: u64, since: u64) -> Option<bool> {
    let mut record = record()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let epoch = record.epoch;
    imp::harvest(base, len, epoch, &mut record.written)?;
    let first = base & !(PAGE - 1);
    let end = base.saturating_add(len);
    Some(
        record
            .written
            .range(first..end)
            .any(|(_, harvested)| *harvested > since),
    )
}

#[cfg(windows)]
mod imp {
    use std::collections::BTreeMap;
    use windows_sys::Win32::System::Memory::{
        GetWriteWatch, MEMORY_BASIC_INFORMATION, VirtualQuery,
    };

    /// `WRITE_WATCH_FLAG_RESET`, `1` - declared under `Win32_System_SystemServices`
    /// (`SystemServices/mod.rs:5047` in 0.61.2), as `MEM_WRITE_WATCH` is.
    const WRITE_WATCH_FLAG_RESET: u32 = 1;

    /// Addresses one call hands back at most.
    const BATCH: usize = 512;

    /// Moves every page of `[base, base + len)` the host has seen written since its last harvest
    /// into `written` at `epoch`, resetting the host's record of them. `None`, having moved what it
    /// could, when part of the range is not write-watched.
    pub(super) fn harvest(
        base: u64,
        len: u64,
        epoch: u64,
        written: &mut BTreeMap<u64, u64>,
    ) -> Option<()> {
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
                    written.insert(*page as u64, epoch);
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
    use std::collections::BTreeMap;

    /// No watch away from Windows yet: every asker compares the bytes.
    pub(super) fn harvest(
        _base: u64,
        _len: u64,
        _epoch: u64,
        _written: &mut BTreeMap<u64, u64>,
    ) -> Option<()> {
        None
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
