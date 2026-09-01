//! Fixed-address reservation, per platform.
//!
//! Only two primitives are needed, and both must **fail rather than relocate**. A
//! guest that asked for an address and silently got a different one corrupts itself in
//! ways that look like anything except a mapping bug.
//!
//! # Windows
//!
//! `VirtualAlloc` at an explicit base already has exactly the semantics wanted: it
//! never overwrites an existing reservation and returns null if the range is taken.
//!
//! `VirtualAlloc2` with placeholders, which earlier planning assumed would be needed,
//! solves a different problem: reserving a large region and later splitting it. Worth
//! reaching for when sub-dividing reservations becomes necessary, and unnecessary
//! complexity before then.
//!
//! # Unix
//!
//! Linux has `MAP_FIXED_NOREPLACE`, which fails instead of evicting. Plain `MAP_FIXED`
//! is never correct here - it would unmap whatever was already there, and the failure
//! would surface as guest corruption rather than as a mapping error.
//!
//! Other Unixes lack that flag, so the fallback passes the address as a *hint* and
//! then checks what came back, unmapping and failing if the kernel chose elsewhere.
//! Slightly racy, but it never evicts, which is the property that matters.

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
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
    };

    /// Maps our protection model onto the platform's.
    ///
    /// **Execute is never dropped, and never implies "no access".** Real guest text
    /// segments here carry `p_flags` of `0x1` - execute with the read bit clear - and an
    /// earlier version of this match sent that to `PAGE_NOACCESS`, because it tested
    /// `read` before `execute`. The image then linked perfectly and faulted on its very
    /// first instruction fetch, which looked like a bad entry point rather than a
    /// protection bug (D065).
    ///
    /// Granting read alongside execute is accurate rather than lax: classic x86-64
    /// paging has no execute-without-read, so `PAGE_EXECUTE` and `PAGE_EXECUTE_READ`
    /// behave identically at the hardware level.
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

        // SAFETY: `VirtualAlloc` is safe to call with any address and size; it
        // validates them itself and returns null on failure. It never overwrites an
        // existing reservation, which is the property this whole function depends on -
        // so a conflict surfaces as null rather than as evicted memory.
        let got = unsafe {
            VirtualAlloc(
                addr as *const core::ffi::c_void,
                size,
                MEM_RESERVE | MEM_COMMIT,
                protection_flags(p),
            )
        };

        if got.is_null() {
            return Err(MemError::Conflict { base, len });
        }
        if got as usize != addr {
            // Documented not to happen when a base is given, but assert it rather than
            // trust it: silently accepting a different address is the exact failure
            // this design exists to prevent.
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
        // SAFETY: the range lies inside a reservation this process owns, `old` is a
        // live local, and `VirtualProtect` validates its arguments and reports failure
        // through its return value rather than by faulting.
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
        // SAFETY: `SYSTEM_INFO` is a plain C struct of integers and unions with no
        // validity requirements, so an all-zero value is a legal instance. It is
        // overwritten entirely by the call below.
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

        // MAP_PRIVATE is mandatory: an mmap with neither PRIVATE nor SHARED is
        // EINVAL. Omitting it made every reservation fail on Linux while Windows
        // passed, which is precisely the kind of gap only running it finds.
        //
        // MAP_FIXED_NOREPLACE then fails instead of evicting. Where it does not exist
        // the address is a hint and the result is checked below - never MAP_FIXED,
        // which would unmap whatever was already there.
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
            // Not every mmap failure is a conflict. Reporting EINVAL as "range taken"
            // sends a reader looking for a phantom occupant instead of at the wrong
            // argument that actually caused it (D010).
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
/// **Not the guest page size, and conflating them is a real bug.** Windows rounds a
/// reservation base *down* to 64 KiB, so a page-aligned-but-not-granularity-aligned
/// request comes back at a different address - which this crate then correctly
/// refuses, since silently accepting relocation is exactly what it exists to prevent.
///
/// Unix has no coarser unit than the page for `mmap`, so the two coincide there. Code
/// that only ever ran on Unix would never notice the difference.
pub fn allocation_granularity() -> u64 {
    imp::allocation_granularity()
}

/// Reserves `len` bytes at exactly `base`, or fails.
pub fn reserve(base: u64, len: u64, protection: Protection) -> Result<Reservation, MemError> {
    imp::reserve(base, len, protection)
}

/// Changes the protection of an already-reserved range.
///
/// Separate from [`reserve`] because population and execution want different
/// permissions: an image is written as read-write and only then made executable.
/// Doing it in one step would mean mapping text writable and leaving it that way.
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

    /// An address far from anything a normal process maps, so the test is about the
    /// mechanism rather than about luck.
    const TEST_BASE: u64 = 0x0000_4000_0000_0000;

    #[test]
    fn the_allocation_granularity_is_reported_and_is_a_power_of_two() {
        // Windows reports 64 KiB here while the guest page is 4 KiB. Anything that
        // rounds a base to the page size alone will be refused on Windows and pass on
        // Unix, which is the worst kind of platform difference.
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
        // The property the whole design rests on: conflicts fail loudly. Silent
        // relocation would corrupt a guest in ways that look like anything else.
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
        // Without a working Drop this fails, which is what makes the test meaningful.
        let again = reserve(base, GUEST_PAGE_SIZE, Protection::READ_WRITE);
        assert!(again.is_ok(), "the range should be free again after drop");
    }
}
