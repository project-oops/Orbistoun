//! Guarding guest pages until something touches them (D717).
//!
//! A deferred copy's destination is made inaccessible, so the first read or write of it, by the
//! guest or by host code on its behalf, faults, and the fault handler in [`crate::report`] carries
//! the copy out and resumes the access. The command processor is handed both halves
//! ([`orbistoun_gpu::agc_driver::LazyCopies`]): guard a range, answering its protection, and put
//! that protection back. A range may span several host regions (guest direct memory is mapped in
//! views of its own), so protection is changed a region at a time. Away from Windows there is no
//! guard, and every copy is carried out when it is made.

/// One change this module made to guest pages' host protection: the range, the
/// protection it asked for, whether the host agreed, and its place in the order of changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Change {
    /// The first byte of the whole pages changed.
    pub base: u64,
    /// Their length.
    pub len: u64,
    /// The protection asked for (`PAGE_*`).
    pub to: u32,
    /// Whether the host made the change.
    pub ok: bool,
    /// Counted from 1, across the run.
    pub sequence: u64,
}

/// How many changes are remembered: the latest, which are the ones a fault follows.
const HISTORY: usize = 256;

/// How many times [`changes_touching`] tries the history's lock before answering that it is busy.
const HISTORY_TRIES: usize = 1000;

static CHANGES: std::sync::Mutex<[Change; HISTORY]> = std::sync::Mutex::new(
    [Change {
        base: 0,
        len: 0,
        to: 0,
        ok: false,
        sequence: 0,
    }; HISTORY],
);
static CHANGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Remembers a change, for a fault report to ask about later.
fn note(base: u64, len: u64, to: u32, ok: bool) {
    let sequence = CHANGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if let Ok(mut changes) = CHANGES.lock() {
        changes[(sequence as usize) % HISTORY] = Change {
            base,
            len,
            to,
            ok,
            sequence,
        };
    }
}

/// The remembered changes that touched `address`, oldest first, into `out` - how many. For a
/// fault report: whether a guest page's protection is one this process set, and
/// whether it was put back. Allocation-free, and it never waits: a fault handler asks it, and a
/// thread that faulted while holding the history answers `None`.
pub fn changes_touching(address: u64, out: &mut [Change]) -> Option<usize> {
    // A bounded retry, not a wait: another thread may hold it for the moment a note takes, and a
    // thread that faulted while holding it itself must still get an answer.
    let changes = (0..HISTORY_TRIES).find_map(|_| {
        CHANGES.try_lock().ok().or_else(|| {
            std::thread::yield_now();
            None
        })
    })?;
    let mut touching = [Change::default(); HISTORY];
    let mut count = 0;
    for change in changes.iter() {
        if change.sequence != 0 && address >= change.base && address - change.base < change.len {
            touching[count] = *change;
            count += 1;
        }
    }
    drop(changes);
    let touching = &mut touching[..count];
    touching.sort_unstable_by_key(|change| change.sequence);
    // The latest ones, oldest first.
    let kept = &touching[count.saturating_sub(out.len())..];
    out[..kept.len()].copy_from_slice(kept);
    Some(kept.len())
}

/// Makes the whole host pages `[base, base + len)` inaccessible, answering the protection they had -
/// which is what [`release`] puts back on all of them. `None`, changing nothing, when any page is not
/// committed, the pages do not all share one protection, or the host refuses.
#[cfg(windows)]
pub fn guard(base: u64, len: u64) -> Option<u32> {
    let to = windows_sys::Win32::System::Memory::PAGE_NOACCESS;
    let had = imp::set_all(base, len, to);
    note(base, len, to, had.is_some());
    had
}

/// Makes the whole host pages `[base, base + len)` read-only, answering the protection they had
/// (D720): a write to them faults, a read does not. `None`, changing nothing, as [`guard`].
#[cfg(windows)]
pub fn protect_writes(base: u64, len: u64) -> Option<u32> {
    let to = windows_sys::Win32::System::Memory::PAGE_READONLY;
    let had = imp::set_all(base, len, to);
    note(base, len, to, had.is_some());
    had
}

/// Puts `protection` back on the whole host pages `[base, base + len)`.
#[cfg(windows)]
pub fn release(base: u64, len: u64, protection: u32) -> bool {
    let Some(pieces) = imp::pieces(base, len) else {
        note(base, len, protection, false);
        return false;
    };
    // Every piece, even after one fails: a page left inaccessible would fault with nothing to
    // answer it.
    let mut ok = true;
    for (address, size, _) in pieces {
        ok &= imp::protect(address, size, protection);
    }
    note(base, len, protection, ok);
    ok
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, VirtualProtect, VirtualQuery,
    };

    /// `[base, base + len)` cut at host region boundaries: each piece's start, length and the
    /// protection its region has. `None` when any of it is not committed memory.
    pub(super) fn pieces(base: u64, len: u64) -> Option<Vec<(usize, usize, u32)>> {
        let start = usize::try_from(base).ok()?;
        let end = start.checked_add(usize::try_from(len).ok()?)?;
        let mut pieces = Vec::new();
        let mut at = start;
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
            let written = unsafe {
                VirtualQuery(
                    std::ptr::with_exposed_provenance::<core::ffi::c_void>(at),
                    &raw mut info,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            let region_end = (info.BaseAddress as usize).saturating_add(info.RegionSize);
            if written == 0 || info.State != MEM_COMMIT || region_end <= at {
                return None;
            }
            let piece_end = region_end.min(end);
            pieces.push((at, piece_end - at, info.Protect));
            at = piece_end;
        }
        Some(pieces)
    }

    /// Sets `new` on every page of `[base, base + len)`, answering the one protection they all had.
    pub(super) fn set_all(base: u64, len: u64, new: u32) -> Option<u32> {
        let pieces = pieces(base, len)?;
        // One protection across the whole range, or restoring it would change the pages that
        // differed.
        let protection = pieces.first()?.2;
        if pieces.iter().any(|piece| piece.2 != protection) {
            return None;
        }
        for (done, &(address, size, _)) in pieces.iter().enumerate() {
            if !protect(address, size, new) {
                // Put back what was changed before the refusal, so a refusal changes nothing.
                for &(address, size, _) in &pieces[..done] {
                    protect(address, size, protection);
                }
                return None;
            }
        }
        Some(protection)
    }

    /// Sets `protection` on `[address, address + size)`, which lies in one host region.
    pub(super) fn protect(address: usize, size: usize, protection: u32) -> bool {
        let mut old = 0u32;
        // SAFETY: the range is committed guest memory inside one host region (from `pieces`), which
        // the command processor was allowed to write; changing its protection changes no bytes, and
        // only the deferred copy that guarded it restores it.
        let ok = unsafe {
            VirtualProtect(
                std::ptr::with_exposed_provenance_mut::<core::ffi::c_void>(address),
                size,
                protection,
                &raw mut old,
            )
        };
        ok != 0
    }
}

/// No guard away from Windows: every copy is carried out when it is made.
#[cfg(not(windows))]
pub fn guard(_base: u64, _len: u64) -> Option<u32> {
    None
}

/// No write protection away from Windows: every target is compared.
#[cfg(not(windows))]
pub fn protect_writes(_base: u64, _len: u64) -> Option<u32> {
    None
}

/// Nothing is ever guarded away from Windows, so nothing is released.
#[cfg(not(windows))]
pub fn release(_base: u64, _len: u64, _protection: u32) -> bool {
    false
}

#[cfg(all(test, windows))]
mod tests {
    /// A range across two neighbouring host allocations, as guest direct memory's views lie, is
    /// guarded whole and released whole, and the change history records both.
    #[test]
    fn a_range_across_two_allocations_is_guarded_and_released_whole() {
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
            VirtualFree,
        };
        const PAGE: usize = 0x1_0000;
        // SAFETY: reserves and commits two fresh allocation granules at an address of the host's
        // choosing; released at the end.
        let first =
            unsafe { VirtualAlloc(std::ptr::null(), 2 * PAGE, MEM_RESERVE, PAGE_READWRITE) };
        assert!(!first.is_null());
        // SAFETY: the reservation is released and taken again as two allocations, one granule each,
        // so the range is two host regions side by side.
        unsafe { VirtualFree(first, 0, MEM_RELEASE) };
        let base = first as usize;
        let allocate = |at: usize| {
            // SAFETY: one granule of the space just released, reserved and committed afresh.
            unsafe {
                VirtualAlloc(
                    std::ptr::with_exposed_provenance(at),
                    PAGE,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_READWRITE,
                )
            }
        };
        let (a, b) = (allocate(base), allocate(base + PAGE));
        assert!(!a.is_null() && !b.is_null(), "two neighbouring allocations");
        let pieces = super::imp::pieces(base as u64, 2 * PAGE as u64).expect("committed");
        assert_eq!(pieces.len(), 2, "two regions: {pieces:x?}");
        assert_eq!(
            super::guard(base as u64, 2 * PAGE as u64),
            Some(PAGE_READWRITE)
        );
        let guarded = super::imp::pieces(base as u64, 2 * PAGE as u64).expect("committed");
        assert!(
            guarded.iter().all(|piece| piece.2 == PAGE_NOACCESS),
            "{guarded:x?}"
        );
        assert!(super::release(base as u64, 2 * PAGE as u64, PAGE_READWRITE));
        let mut changes = [super::Change::default(); 4];
        // Asked until it answers: the history never waits for its lock.
        let count = std::iter::repeat_with(|| {
            super::changes_touching(base as u64 + PAGE as u64, &mut changes)
        })
        .take(1000)
        .find_map(|answer| answer)
        .expect("readable once nothing holds it");
        assert!(count >= 2, "{changes:?}");
        let (guarded, released) = (changes[count - 2], changes[count - 1]);
        assert_eq!((guarded.to, guarded.ok), (PAGE_NOACCESS, true));
        assert_eq!((released.to, released.ok), (PAGE_READWRITE, true));
        assert!(guarded.sequence < released.sequence);
        let released = super::imp::pieces(base as u64, 2 * PAGE as u64).expect("committed");
        assert!(
            released.iter().all(|piece| piece.2 == PAGE_READWRITE),
            "{released:x?}"
        );
        for allocation in [a, b] {
            // SAFETY: the allocation is this test's, and nothing refers to it now.
            unsafe { VirtualFree(allocation, 0, MEM_RELEASE) };
        }
    }
}
