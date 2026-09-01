//! The guest address space.
//!
//! A guest module is linked to load at specific addresses and its allocator hands
//! out addresses the guest's own code then dereferences directly. So orbistoun does
//! not get to choose the layout: it has to reserve exactly what the guest expects
//! inside the host process, or nothing above this layer works.
//!
//! # Why this is the second thing built, not the last
//!
//! Every subsystem shim above depends on it. An audio stub is never reached until
//! the guest has loaded, allocated, and spawned threads, so getting this wrong
//! makes every higher-level trace meaningless.
//!
//! # The two primitives
//!
//! Fixed-address reservation has no portable API, and only two calls are needed:
//!
//! - Unix: `mmap` with `MAP_FIXED_NOREPLACE`, which fails rather than silently
//!   evicting an existing mapping. Plain `MAP_FIXED` is never correct here - it
//!   would unmap host memory and the failure would look like guest corruption.
//! - Windows: `VirtualAlloc2` with a placeholder reservation, which is the only
//!   way to get a specific address range with an explicit conflict error.
//!
//! # Status
//!
//! Implemented and verified on both platforms (D055). [`platform`] holds the
//! primitives; [`AddressSpace::validate`] holds the rules, which stay testable
//! without touching the host address space at all.

pub mod platform;
pub mod stack;
pub mod test_bases;

pub use platform::{Reservation, allocation_granularity};

use orbistoun_core::{DIRECT_MEMORY_ALIGN, GUEST_PAGE_SIZE};

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
    /// The bit values are fixed by the format: execute is 1, write is 2, read is 4.
    /// Note the ordering is *not* the intuitive read-write-execute - transcribing them
    /// in the wrong order maps text as writable data, which then faults on the first
    /// instruction fetch with nothing to point at the cause.
    pub const fn from_elf_flags(flags: u32) -> Self {
        Self {
            read: flags & 0x4 != 0,
            write: flags & 0x2 != 0,
            execute: flags & 0x1 != 0,
        }
    }

    /// Whether this permits writing and executing at once.
    ///
    /// Worth asking about explicitly. A segment mapped both ways is a legitimate thing
    /// for a guest to request and a real hazard, so it is reported rather than either
    /// silently honoured or silently downgraded (D060).
    pub const fn is_writable_and_executable(&self) -> bool {
        self.write && self.execute
    }

    /// Every permission this and `other` grant between them.
    ///
    /// Needed because segments may share a page: the page must satisfy both, and a
    /// loader that simply applies the last one strips permissions the first still
    /// needs.
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
    /// Guest base address, which equals the host address - the mapping is
    /// identity, because guest code dereferences these values directly.
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
    /// Separated from the mapping itself so the rules are testable without
    /// touching the host address space at all - which is what lets this crate
    /// have meaningful tests before the platform code exists.
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
    /// Fails rather than relocating. A guest that asked for an address and got a
    /// different one will corrupt itself in ways that look like anything except a
    /// mapping bug.
    pub fn reserve(
        &mut self,
        base: u64,
        len: u64,
        protection: Protection,
    ) -> Result<Region, MemError> {
        self.validate(base, len, false)?;
        let held = platform::reserve(base, len, protection)?;
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
    /// Exposed so a caller that means to **commit into an existing reservation** - a guest
    /// that reserved a range with one call and then mapped physical memory inside it with
    /// another - can tell that case from a fresh mapping without reaching into the region
    /// list itself. Reserving the same range a second time conflicts (that is the point of
    /// [`reserve`](Self::reserve) refusing to relocate), so the two operations have to be
    /// told apart before the mapping, not after it fails.
    #[must_use]
    pub fn owns(&self, base: u64, len: u64) -> bool {
        let end = base.saturating_add(len);
        self.regions
            .iter()
            .any(|r| r.base <= base && end <= r.base.saturating_add(r.len))
    }

    /// Changes the protection of a range already covered by a reservation.
    ///
    /// Refuses a range this address space does not own. Calling the platform directly
    /// would let a typo re-protect arbitrary host memory - including this process's own
    /// code - and the failure would appear as an unrelated crash.
    pub fn protect(&mut self, base: u64, len: u64, protection: Protection) -> Result<(), MemError> {
        if !self.owns(base, len) {
            let end = base.saturating_add(len);
            return Err(MemError::HostRefused(format!(
                "{base:#x}..{end:#x} is not inside any region this address space owns"
            )));
        }
        platform::protect(base, len, protection)
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
        // Execute is 1 and read is 4, which is the reverse of how they are usually
        // spoken. Getting it backwards maps text as writable data and faults on the
        // first instruction fetch with nothing pointing at the cause.
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
        // Two segments sharing a page must both still work. Applying the last one
        // strips permissions the first still needs.
        let both = Protection::READ_EXECUTE.union(Protection::READ_WRITE);
        assert!(both.read && both.write && both.execute);
    }

    #[test]
    fn protecting_a_range_outside_every_region_is_refused() {
        // Passing this through to the platform would let a typo re-protect arbitrary
        // host memory, including this process's own code.
        let mut s = space_with(0x1_0000, GUEST_PAGE_SIZE * 4);
        assert!(
            s.protect(0x9_0000, GUEST_PAGE_SIZE, Protection::READ_ONLY)
                .is_err()
        );
    }
}
