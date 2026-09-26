//! The guest address space.
//!
//! A guest module is linked to load at specific addresses and its code dereferences what its
//! allocator hands out, so orbistoun reserves exactly the layout the guest expects inside the
//! host process. Fixed-address reservation uses `mmap` with `MAP_FIXED_NOREPLACE` on Unix
//! (plain `MAP_FIXED` would silently evict host mappings) and `VirtualAlloc2` placeholder
//! reservations on Windows. [`platform`] holds the primitives; [`AddressSpace::validate`]
//! holds the rules, testable without touching the host address space.

pub mod blocks;
pub mod guest;
pub mod platform;
pub mod stack;
pub mod test_bases;
pub mod watch;

/// A base no other test in this crate gets: one range for the crate, one cursor for every test
/// module that takes from it (D324).
#[cfg(test)]
pub(crate) fn unique_test_base() -> u64 {
    use test_bases::{Range, crates};
    static RANGE: Range = Range::nth(crates::MEM);
    RANGE.take()
}

pub use platform::{Reservation, allocation_granularity};

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use orbistoun_core::{DIRECT_MEMORY_ALIGN, GUEST_PAGE_SIZE};

/// The base of the last reservation this process failed to make, with [`FAIL_KIND`] the
/// reason and [`FAIL_LEN`] the size.
///
/// A failed reservation otherwise reaches the guest only as `NoMemory`, and the guest faults
/// far away on the null it kept. Recorded allocation-free, because `reserve` runs on the
/// guest's stack when a guest maps memory (D381).
static FAIL_BASE: AtomicU64 = AtomicU64::new(0);
/// The size of the last failed reservation; see [`FAIL_BASE`].
static FAIL_LEN: AtomicU64 = AtomicU64::new(0);
/// Why the last reservation failed: `0` none yet, `1` conflict, `2` misaligned, `3` host
/// refused. Written last, with `Release`, so a reader that sees it also sees the base and len.
static FAIL_KIND: AtomicU8 = AtomicU8::new(0);

/// How many reservations have failed, so two runs can be compared on more than their last
/// failure.
static FAIL_COUNT: AtomicU64 = AtomicU64::new(0);

/// The base of the first failure. Many failures at one address are a caller retrying; at
/// different addresses, a walk running out of room.
static FIRST_FAIL_BASE: AtomicU64 = AtomicU64::new(0);

/// The last reservation this process could not make, for the run report to surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReserveFailure {
    /// The base address the reservation was attempted at.
    pub base: u64,
    /// The length in bytes.
    pub len: u64,
    /// Why it failed, in words a report can print.
    pub reason: &'static str,
}

/// Records a reservation failure for the run report. Allocation-free and lock-free (D381).
fn note_reserve_failure(base: u64, len: u64, error: &MemError) {
    FAIL_BASE.store(base, Ordering::Relaxed);
    FAIL_LEN.store(len, Ordering::Relaxed);
    let kind = match error {
        MemError::Conflict { .. } => 1,
        MemError::Misaligned { .. } => 2,
        MemError::HostRefused(_) => 3,
    };
    if FAIL_COUNT.fetch_add(1, Ordering::Relaxed) == 0 {
        FIRST_FAIL_BASE.store(base, Ordering::Relaxed);
    }
    // Last, so a reader never sees the reason set against a stale base or len.
    FAIL_KIND.store(kind, Ordering::Release);
}

/// How many reservations this process failed to make.
///
/// Zero agrees with [`None`] from [`last_reserve_failure`]; after several failures that
/// reports only the newest.
#[must_use]
pub fn reserve_failures() -> u64 {
    FAIL_COUNT.load(Ordering::Relaxed)
}

/// The base of the first reservation that failed, or [`None`] if none has.
#[must_use]
pub fn first_reserve_failure_base() -> Option<u64> {
    (FAIL_COUNT.load(Ordering::Relaxed) > 0).then(|| FIRST_FAIL_BASE.load(Ordering::Relaxed))
}

/// The last reservation this process failed to make, or [`None`] if every one has succeeded.
///
/// Read from the host side at the end of a run, so it may allocate.
#[must_use]
pub fn last_reserve_failure() -> Option<ReserveFailure> {
    let reason = match FAIL_KIND.load(Ordering::Acquire) {
        1 => "conflict - the address was already reserved (something else holds it)",
        2 => "misaligned - the base or length broke an ABI alignment rule",
        3 => "host refused - out of memory, or an address the host would not give",
        _ => return None,
    };
    Some(ReserveFailure {
        base: FAIL_BASE.load(Ordering::Relaxed),
        len: FAIL_LEN.load(Ordering::Relaxed),
        reason,
    })
}

/// Why an address-space operation failed.
#[derive(Debug, thiserror::Error)]
pub enum MemError {
    /// The requested range overlaps something already mapped.
    #[error("range {base:#x}..{:#x} conflicts with an existing mapping", base + len)]
    Conflict {
        /// Start of the requested range.
        base: u64,
        /// Length in bytes.
        len: u64,
    },
    /// The request violated an ABI alignment rule.
    #[error("{what} must be {align:#x}-aligned, got {value:#x}")]
    Misaligned {
        /// Which value was wrong.
        what: &'static str,
        /// Required alignment.
        align: u64,
        /// The offending value.
        value: u64,
    },
    /// The host refused the mapping.
    #[error("host rejected the mapping: {0}")]
    HostRefused(String),
}

/// How a guest region may be accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Protection {
    /// Readable by the guest.
    pub read: bool,
    /// Writable by the guest.
    pub write: bool,
    /// Executable by the guest. Set for loaded code segments.
    pub execute: bool,
}

impl Protection {
    /// Read plus write, the common case for data.
    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
        execute: false,
    };
    /// Read plus execute, for a loaded text segment.
    pub const READ_EXECUTE: Self = Self {
        read: true,
        write: false,
        execute: true,
    };
    /// Read only, for constant data and relocated-then-sealed regions.
    pub const READ_ONLY: Self = Self {
        read: true,
        write: false,
        execute: false,
    };

    /// Interprets an ELF program header's `p_flags`.
    ///
    /// The format fixes execute as 1, write as 2 and read as 4; reversing them maps text as
    /// writable data, which faults on the first instruction fetch.
    pub const fn from_elf_flags(flags: u32) -> Self {
        Self {
            read: flags & 0x4 != 0,
            write: flags & 0x2 != 0,
            execute: flags & 0x1 != 0,
        }
    }

    /// Whether this permits writing and executing at once.
    ///
    /// A guest may legitimately request it, and it is a hazard, so it is reported rather than
    /// honoured or downgraded silently.
    pub const fn is_writable_and_executable(&self) -> bool {
        self.write && self.execute
    }

    /// Every permission this and `other` grant between them.
    ///
    /// Segments may share a page, and the page must satisfy both.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self {
            read: self.read || other.read,
            write: self.write || other.write,
            execute: self.execute || other.execute,
        }
    }
}

/// One reserved guest region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// Guest base address, which equals the host address: guest code dereferences these values
    /// directly.
    pub base: u64,
    /// Length in bytes, always a multiple of [`GUEST_PAGE_SIZE`].
    pub len: u64,
    /// Access permitted.
    pub protection: Protection,
}

/// The guest's address space within this process.
///
/// Holds the reservations as well as the bookkeeping, so a region stays mapped for
/// exactly as long as the address space that owns it.
#[derive(Debug, Default)]
pub struct AddressSpace {
    regions: Vec<Region>,
    held: Vec<Reservation>,
}

impl AddressSpace {
    /// Creates an empty address space.
    pub const fn new() -> Self {
        Self {
            regions: Vec::new(),
            held: Vec::new(),
        }
    }

    /// Every region reserved so far, in insertion order.
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// Checks a request against the ABI rules and existing reservations.
    ///
    /// Separate from the mapping so the rules are testable without touching the host address
    /// space.
    pub fn validate(&self, base: u64, len: u64, direct: bool) -> Result<(), MemError> {
        if len == 0 || len % GUEST_PAGE_SIZE != 0 {
            return Err(MemError::Misaligned {
                what: "length",
                align: GUEST_PAGE_SIZE,
                value: len,
            });
        }
        let align = if direct {
            DIRECT_MEMORY_ALIGN
        } else {
            GUEST_PAGE_SIZE
        };
        if base % align != 0 {
            return Err(MemError::Misaligned {
                what: "base address",
                align,
                value: base,
            });
        }
        let end = base.saturating_add(len);
        for r in &self.regions {
            let r_end = r.base.saturating_add(r.len);
            if base < r_end && r.base < end {
                return Err(MemError::Conflict { base, len });
            }
        }
        Ok(())
    }

    /// Reserves `len` bytes at exactly `base`.
    ///
    /// Fails rather than relocating: a guest given a different address than it asked for corrupts
    /// itself in ways that do not look like a mapping fault.
    pub fn reserve(
        &mut self,
        base: u64,
        len: u64,
        protection: Protection,
    ) -> Result<Region, MemError> {
        // Both the validate conflict and the host refusal pass through here, so the failure is
        // recorded with its base and reason.
        if let Err(e) = self.validate(base, len, false) {
            note_reserve_failure(base, len, &e);
            return Err(e);
        }
        let held = match platform::reserve(base, len, protection) {
            Ok(held) => held,
            Err(e) => {
                note_reserve_failure(base, len, &e);
                return Err(e);
            }
        };
        let region = Region {
            base,
            len,
            protection,
        };
        self.regions.push(region);
        self.held.push(held);
        Ok(region)
    }

    /// Whether `[base, base + len)` lies entirely within a single region this space reserved.
    ///
    /// Lets a caller that commits into an existing reservation (reserve with one call, map
    /// inside it with another) tell that case from a fresh mapping before mapping, since
    /// reserving the same range twice conflicts (D460).
    #[must_use]
    pub fn owns(&self, base: u64, len: u64) -> bool {
        let end = base.saturating_add(len);
        self.regions
            .iter()
            .any(|r| r.base <= base && end <= r.base.saturating_add(r.len))
    }

    /// Changes the protection of a range already covered by a reservation.
    ///
    /// Refuses a range this address space does not own, so a bad address cannot re-protect host
    /// memory, including this process's own code.
    pub fn protect(&mut self, base: u64, len: u64, protection: Protection) -> Result<(), MemError> {
        if !self.owns(base, len) {
            let end = base.saturating_add(len);
            let error = MemError::HostRefused(format!(
                "{base:#x}..{end:#x} is not inside any region this address space owns"
            ));
            note_reserve_failure(base, len, &error);
            return Err(error);
        }
        // Recorded for the same reason as in `reserve`: the guest otherwise sees only `NoMemory`.
        if let Err(e) = platform::protect(base, len, protection) {
            note_reserve_failure(base, len, &e);
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AddressSpace, MemError, Protection, Region};
    use orbistoun_core::GUEST_PAGE_SIZE;

    fn space_with(base: u64, len: u64) -> AddressSpace {
        let mut s = AddressSpace::new();
        s.regions.push(Region {
            base,
            len,
            protection: Protection::READ_WRITE,
        });
        s
    }

    #[test]
    fn a_range_inside_a_reservation_is_owned() {
        // The reserve-then-map case: a sub-range of an already-reserved region is committed
        // into rather than reserved a second time (D460).
        let s = space_with(0x7200_0000_0000, 0x10 * GUEST_PAGE_SIZE);
        assert!(s.owns(0x7200_0000_0000, GUEST_PAGE_SIZE), "its own base");
        assert!(
            s.owns(0x7200_0000_0000 + 2 * GUEST_PAGE_SIZE, GUEST_PAGE_SIZE),
            "a page partway into it"
        );
    }

    #[test]
    fn a_range_outside_or_straddling_a_reservation_is_not_owned() {
        let s = space_with(0x7200_0000_0000, 0x10 * GUEST_PAGE_SIZE);
        assert!(
            !s.owns(0x7100_0000_0000, GUEST_PAGE_SIZE),
            "wholly below it"
        );
        assert!(
            !s.owns(
                0x7200_0000_0000 + 0x0f * GUEST_PAGE_SIZE,
                2 * GUEST_PAGE_SIZE
            ),
            "running off the end is not owned - a partial commit must not read as a full one"
        );
        assert!(
            !AddressSpace::new().owns(0x7200_0000_0000, GUEST_PAGE_SIZE),
            "an empty space owns nothing"
        );
    }

    #[test]
    fn rejects_unaligned_length() {
        let s = AddressSpace::new();
        assert!(matches!(
            s.validate(0x1_0000, 1, false),
            Err(MemError::Misaligned { what: "length", .. })
        ));
    }

    #[test]
    fn rejects_unaligned_base() {
        let s = AddressSpace::new();
        assert!(matches!(
            s.validate(0x1001, GUEST_PAGE_SIZE, false),
            Err(MemError::Misaligned {
                what: "base address",
                ..
            })
        ));
    }

    #[test]
    fn direct_memory_demands_stricter_alignment() {
        let s = AddressSpace::new();
        // Page-aligned but not direct-memory-aligned: fine for flexible memory,
        // a corruption source for direct memory.
        assert!(s.validate(GUEST_PAGE_SIZE, GUEST_PAGE_SIZE, false).is_ok());
        assert!(matches!(
            s.validate(GUEST_PAGE_SIZE, GUEST_PAGE_SIZE, true),
            Err(MemError::Misaligned { .. })
        ));
    }

    #[test]
    fn detects_overlap_in_both_directions() {
        let s = space_with(0x10_0000, 0x1_0000);
        // Overlapping start, overlapping end, and full containment must all fail.
        for (base, len) in [
            (0x10_8000_u64, 0x1_0000_u64),
            (0x0F_8000, 0x1_0000),
            (0x10_2000, 0x1000),
        ] {
            assert!(
                matches!(s.validate(base, len, false), Err(MemError::Conflict { .. })),
                "expected conflict for {base:#x}+{len:#x}"
            );
        }
        // Exactly abutting is not overlapping.
        assert!(s.validate(0x11_0000, 0x1000, false).is_ok());
    }
    #[test]
    fn elf_flag_bits_map_to_the_right_permissions() {
        // Execute is 1 and read is 4, the reverse of the usual spoken order.
        assert_eq!(Protection::from_elf_flags(0x4), Protection::READ_ONLY);
        assert_eq!(
            Protection::from_elf_flags(0x4 | 0x2),
            Protection::READ_WRITE
        );
        assert_eq!(
            Protection::from_elf_flags(0x4 | 0x1),
            Protection::READ_EXECUTE
        );
        assert_eq!(
            Protection::from_elf_flags(0x0),
            Protection {
                read: false,
                write: false,
                execute: false,
            }
        );
    }

    #[test]
    fn a_write_execute_segment_is_reported_rather_than_quietly_altered() {
        let rwx = Protection::from_elf_flags(0x4 | 0x2 | 0x1);
        assert!(rwx.is_writable_and_executable());
        assert!(!Protection::READ_EXECUTE.is_writable_and_executable());
        assert!(!Protection::READ_WRITE.is_writable_and_executable());
    }

    #[test]
    fn union_keeps_every_permission_either_side_needs() {
        // Two segments sharing a page must both still work.
        let both = Protection::READ_EXECUTE.union(Protection::READ_WRITE);
        assert!(both.read && both.write && both.execute);
    }

    #[test]
    fn protecting_a_range_outside_every_region_is_refused() {
        // Passing this to the platform could re-protect arbitrary host memory.
        let mut s = space_with(0x1_0000, GUEST_PAGE_SIZE * 4);
        assert!(
            s.protect(0x9_0000, GUEST_PAGE_SIZE, Protection::READ_ONLY)
                .is_err()
        );
    }
}
