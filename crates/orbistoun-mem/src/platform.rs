//! Fixed-address reservation, per platform.
//!
//! Both primitives fail rather than relocate: a guest given a different address than it asked
//! for corrupts itself in ways that do not look like a mapping fault. On Windows,
//! `VirtualAlloc` at an explicit base never overwrites an existing reservation and returns
//! null if the range is taken. On Linux, `MAP_FIXED_NOREPLACE` fails instead of evicting;
//! other Unixes pass the address as a hint and unmap and fail if the kernel chose elsewhere,
//! which is slightly racy but never evicts. Plain `MAP_FIXED` is never used.

use crate::{MemError, Protection};

/// A reservation held by the platform, released on drop.
#[derive(Debug)]
pub struct Reservation {
    base: u64,
    len: u64,
}

impl Reservation {
    /// Guest base address, which equals the host address.
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// Length in bytes.
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether the reservation covers nothing.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(windows)]
mod imp {
    use super::{MemError, Protection, Reservation};
    use windows_sys::Win32::Foundation::{ERROR_INVALID_ADDRESS, GetLastError};
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
    };

    /// `MEM_WRITE_WATCH`, `0x200000` - `windows-sys` declares it under
    /// `Win32_System_SystemServices` (`SystemServices/mod.rs:2702` in 0.61.2), a feature this crate
    /// does not otherwise need.
    const MEM_WRITE_WATCH: u32 = 0x0020_0000;

    /// Maps our protection model onto the platform's.
    ///
    /// Execute is never dropped: guest text segments carry `p_flags` of `0x1` (execute, read
    /// clear), and mapping that to `PAGE_NOACCESS` faults on the first instruction fetch. Granting
    /// read with execute is accurate, since x86-64 paging has no execute-without-read.
    const fn protection_flags(p: Protection) -> u32 {
        match (p.read, p.write, p.execute) {
            (_, true, true) => PAGE_EXECUTE_READWRITE,
            (_, true, false) => PAGE_READWRITE,
            (_, false, true) => PAGE_EXECUTE_READ,
            (true, false, false) => PAGE_READONLY,
            _ => PAGE_NOACCESS,
        }
    }

    pub(super) fn reserve(base: u64, len: u64, p: Protection) -> Result<Reservation, MemError> {
        let addr = usize::try_from(base).map_err(|_| {
            MemError::HostRefused("base address does not fit in a pointer".to_owned())
        })?;
        let size = usize::try_from(len)
            .map_err(|_| MemError::HostRefused("length does not fit in a pointer".to_owned()))?;

        // SAFETY: `VirtualAlloc` validates any address and size itself and returns null on
        // failure; it never overwrites an existing reservation, so a conflict surfaces as null.
        let got = unsafe {
            // Write-watched, so what has been written since a point can be asked of the host rather
            // than found by comparing every byte (`crate::watch`).
            VirtualAlloc(
                addr as *const core::ffi::c_void,
                size,
                MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH,
                protection_flags(p),
            )
        };

        if got.is_null() {
            // SAFETY: reads this thread's last-error code, set by the `VirtualAlloc` that
            // just failed; it takes no arguments and cannot fault.
            let code = unsafe { GetLastError() };
            // `ERROR_INVALID_ADDRESS` means the base is already reserved, a genuine conflict.
            // Anything else is the host refusing, and is reported as that rather than as a taken
            // range (D010).
            return Err(if code == ERROR_INVALID_ADDRESS {
                MemError::Conflict { base, len }
            } else {
                MemError::HostRefused(format!(
                    "VirtualAlloc({base:#x}, {len:#x}) refused: error {code}"
                ))
            });
        }
        if got as usize != addr {
            // Documented not to happen when a base is given; checked anyway, since accepting a
            // different address is the failure this design prevents.
            // SAFETY: `got` was returned by VirtualAlloc and has not been freed.
            unsafe { VirtualFree(got, 0, MEM_RELEASE) };
            return Err(MemError::HostRefused(format!(
                "requested {base:#x}, kernel returned {:#x}",
                got as usize
            )));
        }
        Ok(Reservation { base, len })
    }

    pub(super) fn protect(base: u64, len: u64, p: Protection) -> Result<(), MemError> {
        let addr = usize::try_from(base)
            .map_err(|_| MemError::HostRefused("base does not fit in a pointer".to_owned()))?;
        let size = usize::try_from(len)
            .map_err(|_| MemError::HostRefused("length does not fit in a pointer".to_owned()))?;
        let mut old: u32 = 0;
        // SAFETY: the range lies inside a reservation this process owns, `old` is a live local,
        // and `VirtualProtect` reports failure through its return value.
        let ok = unsafe {
            VirtualProtect(
                addr as *mut core::ffi::c_void,
                size,
                protection_flags(p),
                &raw mut old,
            )
        };
        if ok == 0 {
            return Err(MemError::HostRefused(format!(
                "VirtualProtect refused {base:#x}..{:#x}",
                base.saturating_add(len)
            )));
        }
        Ok(())
    }

    pub(super) fn allocation_granularity() -> u64 {
        use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
        // SAFETY: `SYSTEM_INFO` is a plain C struct of integers and unions, so all-zero is a legal
        // value; the call below overwrites it.
        let mut info: SYSTEM_INFO = unsafe { core::mem::zeroed() };
        // SAFETY: `GetSystemInfo` fills the struct it is given and cannot fail; the
        // pointer refers to a live, correctly sized local.
        unsafe { GetSystemInfo(&raw mut info) };
        u64::from(info.dwAllocationGranularity)
    }

    pub(super) fn release(r: &Reservation) {
        let Ok(addr) = usize::try_from(r.base) else {
            return;
        };
        // SAFETY: the address came from a successful VirtualAlloc in `reserve` and is
        // released exactly once, from `Drop`. MEM_RELEASE requires a size of zero.
        unsafe {
            VirtualFree(addr as *mut core::ffi::c_void, 0, MEM_RELEASE);
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::{MemError, Protection, Reservation};
    use rustix::mm::{MapFlags, ProtFlags, mmap_anonymous, munmap};

    const fn protection_flags(p: Protection) -> ProtFlags {
        let mut f = ProtFlags::empty();
        if p.read {
            f = f.union(ProtFlags::READ);
        }
        if p.write {
            f = f.union(ProtFlags::WRITE);
        }
        if p.execute {
            f = f.union(ProtFlags::EXEC);
        }
        f
    }

    pub(super) fn reserve(base: u64, len: u64, p: Protection) -> Result<Reservation, MemError> {
        let addr = usize::try_from(base)
            .map_err(|_| MemError::HostRefused("base does not fit in a pointer".to_owned()))?;
        let size = usize::try_from(len)
            .map_err(|_| MemError::HostRefused("length does not fit in a pointer".to_owned()))?;

        // MAP_PRIVATE is mandatory: an mmap with neither PRIVATE nor SHARED is EINVAL.
        // MAP_FIXED_NOREPLACE fails instead of evicting; where it does not exist the address is a
        // hint and the result is checked below. Never MAP_FIXED, which unmaps what was there.
        #[cfg(target_os = "linux")]
        let flags = MapFlags::PRIVATE.union(MapFlags::FIXED_NOREPLACE);
        #[cfg(not(target_os = "linux"))]
        let flags = MapFlags::PRIVATE;

        // SAFETY: an anonymous mapping backs no file and aliases nothing. The address
        // is either refused (Linux, via FIXED_NOREPLACE) or treated as a hint and
        // verified below, so no existing mapping is ever replaced.
        let got = unsafe {
            mmap_anonymous(
                core::ptr::with_exposed_provenance_mut(addr),
                size,
                protection_flags(p),
                flags,
            )
        }
        .map_err(|e| {
            // Not every mmap failure is a conflict: EINVAL is a bad argument, not a taken range
            // (D010).
            match e {
                rustix::io::Errno::EXIST | rustix::io::Errno::NOMEM => {
                    MemError::Conflict { base, len }
                }
                other => MemError::HostRefused(format!("mmap failed: {other}")),
            }
        })?;

        if got.addr() != addr {
            // SAFETY: `got` was returned by the mmap above and has not been unmapped.
            unsafe {
                let _ = munmap(got, size);
            }
            return Err(MemError::Conflict { base, len });
        }
        Ok(Reservation { base, len })
    }

    pub(super) fn protect(base: u64, len: u64, p: Protection) -> Result<(), MemError> {
        use rustix::mm::{MprotectFlags, mprotect};
        let addr = usize::try_from(base)
            .map_err(|_| MemError::HostRefused("base does not fit in a pointer".to_owned()))?;
        let size = usize::try_from(len)
            .map_err(|_| MemError::HostRefused("length does not fit in a pointer".to_owned()))?;
        let mut f = MprotectFlags::empty();
        if p.read {
            f = f.union(MprotectFlags::READ);
        }
        if p.write {
            f = f.union(MprotectFlags::WRITE);
        }
        if p.execute {
            f = f.union(MprotectFlags::EXEC);
        }
        // SAFETY: the range lies inside a reservation this process owns and remains
        // mapped for the lifetime of the owning `AddressSpace`.
        unsafe { mprotect(core::ptr::with_exposed_provenance_mut(addr), size, f) }
            .map_err(|e| MemError::HostRefused(format!("mprotect failed: {e}")))
    }

    pub(super) fn allocation_granularity() -> u64 {
        // mmap has no unit coarser than the page.
        rustix::param::page_size() as u64
    }

    pub(super) fn release(r: &Reservation) {
        let (Ok(addr), Ok(size)) = (usize::try_from(r.base), usize::try_from(r.len)) else {
            return;
        };
        // SAFETY: the address and length come from a successful mmap in `reserve` and
        // are unmapped exactly once, from `Drop`.
        unsafe {
            let _ = munmap(core::ptr::with_exposed_provenance_mut(addr), size);
        }
    }
}

/// The coarsest alignment a reservation base must satisfy on this host.
///
/// Not the guest page size. Windows rounds a reservation base down to 64 KiB, so a request
/// aligned only to the page comes back elsewhere and is refused. On Unix the two coincide.
pub fn allocation_granularity() -> u64 {
    imp::allocation_granularity()
}

/// Reserves `len` bytes at exactly `base`, or fails.
pub fn reserve(base: u64, len: u64, protection: Protection) -> Result<Reservation, MemError> {
    imp::reserve(base, len, protection)
}

/// Changes the protection of an already-reserved range.
///
/// Separate from [`reserve`] because an image is written read-write and only then made
/// executable, so text is never left writable.
pub fn protect(base: u64, len: u64, protection: Protection) -> Result<(), MemError> {
    imp::protect(base, len, protection)
}

impl Drop for Reservation {
    fn drop(&mut self) {
        imp::release(self);
    }
}

#[cfg(test)]
mod tests {
    use super::reserve;
    use crate::Protection;
    use orbistoun_core::GUEST_PAGE_SIZE;

    /// An address far from anything a normal process maps.
    const TEST_BASE: u64 = 0x0000_4000_0000_0000;

    #[test]
    fn the_allocation_granularity_is_reported_and_is_a_power_of_two() {
        // Windows reports 64 KiB here while the guest page is 4 KiB; rounding a base to the page
        // alone is refused on Windows and passes on Unix.
        let g = super::allocation_granularity();
        assert!(g >= GUEST_PAGE_SIZE, "granularity {g} below a page");
        assert!(g.is_power_of_two(), "granularity {g} is not a power of two");
    }

    #[test]
    fn a_reservation_lands_at_exactly_the_requested_address() {
        let r = reserve(TEST_BASE, GUEST_PAGE_SIZE * 4, Protection::READ_WRITE)
            .expect("a fresh high address should be available");
        assert_eq!(r.base(), TEST_BASE, "relocation is never acceptable");
        assert_eq!(r.len(), GUEST_PAGE_SIZE * 4);
        assert!(!r.is_empty());
    }

    #[test]
    fn reserved_memory_is_actually_writable() {
        let r = reserve(
            TEST_BASE + 0x1_0000_0000,
            GUEST_PAGE_SIZE,
            Protection::READ_WRITE,
        )
        .expect("reserve");
        let p =
            core::ptr::with_exposed_provenance_mut::<u8>(usize::try_from(r.base()).expect("fits"));
        // SAFETY: the reservation succeeded at exactly this address with write
        // permission and covers this byte, so the write is in bounds and owned.
        unsafe { p.write_volatile(0xAB) };
        // SAFETY: same reservation, same byte, written immediately above.
        let read_back = unsafe { p.read_volatile() };
        assert_eq!(read_back, 0xAB);
    }

    #[test]
    fn a_second_reservation_of_the_same_range_is_refused_not_silently_moved() {
        // Conflicts fail loudly rather than relocating.
        let base = TEST_BASE + 0x2_0000_0000;
        let _held = reserve(base, GUEST_PAGE_SIZE, Protection::READ_WRITE).expect("first");
        let second = reserve(base, GUEST_PAGE_SIZE, Protection::READ_WRITE);
        assert!(second.is_err(), "the range is taken; the second must fail");
    }

    #[test]
    fn dropping_a_reservation_frees_the_range_for_reuse() {
        let base = TEST_BASE + 0x3_0000_0000;
        {
            let _r = reserve(base, GUEST_PAGE_SIZE, Protection::READ_WRITE).expect("first");
        }
        // Fails unless Drop released the first reservation.
        let again = reserve(base, GUEST_PAGE_SIZE, Protection::READ_WRITE);
        assert!(again.is_ok(), "the range should be free again after drop");
    }
}
