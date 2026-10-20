//! Memory pools: physical memory set aside with `sceKernelMemoryPoolExpand`, address space
//! reserved for it with `sceKernelMemoryPoolReserve`, and granules backed and released inside
//! that space with `sceKernelMemoryPoolCommit` and `sceKernelMemoryPoolDecommit`.
//!
//! The console's answers are obSCEne's `020-memory/memory-pool-prt-aperture` rows
//! (REQ-20261012T1822Z-7389, report `report-20261014-batch1.log` lines 3463-3494, FW 12.40):
//!
//! - Expand of 4 MiB at 64 KiB alignment answers 0 and writes a physical address. Length 0 and
//!   alignment 0 answer 0; length `0x1000` and alignment `0x3000` answer `EINVAL`. A null
//!   physical out answers 0.
//! - Reserve answers 0 and writes an address. A fixed reserve over a held pool reservation answers
//!   0. Length 0 answers `EINVAL`. A null out faults.
//! - Commit of one 64 KiB granule answers 0 and the granule reads and writes. A protection query
//!   then answers the whole reservation's bounds with the protection committed.
//! - Decommit answers 0. Less than a granule answers `EINVAL`, and a second decommit of the same
//!   granule answers 0. A read of a decommitted granule faults.
//!
//! Where no row settles an arm, the call refuses by name and REQ-20261020T1310Z-km01 asks the
//! console. Those arms include commit outside a reservation, flags, recommit contents, and the
//! granule of an Expand length. The semantics the console shares with FreeBSD come from there:
//! `MAP_FIXED` replaces what it lands on, and a hint that is held moves up to the first free range
//! (`sys/vm/vm_mmap.c` `kern_mmap` and `vm_map_find`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, OnceLock};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, errno};
use orbistoun_mem::{Protection, guest};

use super::{
    CONFLICT_RETRIES, MAP_FIXED, OK, checked_next_multiple_of, direct, mapping_placed, mappings,
    next_mapping_base, note_requested_protection, protection_from_guest, slot_unit,
};

/// The unit a pool commits and decommits in.
///
/// A decommit of `0x1000` bytes is refused where one of `0x10000` succeeds. Expand at `0x1000` is
/// refused too, so the same unit is taken for Expand and Commit lengths; km01 1a and 1c measure it
/// for each.
pub const GRANULE: u64 = 0x1_0000;

/// Why a pool call is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `EINVAL`, as the console answers a malformed length or alignment.
    Invalid,
    /// A commit the pool's free memory cannot cover.
    NoMemory,
    /// An arm no measurement settles; the text says which, for the log.
    Unmeasured(&'static str),
}

/// One `sceKernelMemoryPoolReserve` range and the granules committed in it.
#[derive(Debug, Default)]
struct Reservation {
    len: u64,
    /// The base of every committed granule.
    committed: BTreeSet<u64>,
}

/// The bookkeeping behind every pool call, with no memory of its own, so its rules are tested
/// without a mapping.
#[derive(Debug, Default)]
pub struct Pool {
    /// Bytes Expand set aside.
    capacity: u64,
    /// Bytes committed across every reservation.
    committed: u64,
    reservations: BTreeMap<u64, Reservation>,
}

/// Whether `[addr, addr + len)` starts and ends on granule boundaries.
const fn granular(addr: u64, len: u64) -> bool {
    addr % GRANULE == 0 && len % GRANULE == 0
}

/// The base of every granule in `[addr, addr + len)`.
fn granules(addr: u64, len: u64) -> impl Iterator<Item = u64> {
    (0..len / GRANULE).map(move |i| addr + i * GRANULE)
}

impl Pool {
    /// Whether Expand accepts `len` at `align`: an alignment must be a power of two, and a length a
    /// whole number of granules. Zero is accepted for both.
    ///
    /// # Errors
    ///
    /// [`Refusal::Invalid`] for the measured refusals. [`Refusal::Unmeasured`] for a power-of-two
    /// alignment below a granule, which km01 1a asks about.
    pub const fn check_expand(len: u64, align: u64) -> Result<(), Refusal> {
        if align != 0 && !align.is_power_of_two() {
            return Err(Refusal::Invalid);
        }
        if align != 0 && align < GRANULE {
            return Err(Refusal::Unmeasured(
                "an Expand alignment below the 64 KiB granule",
            ));
        }
        if len % GRANULE != 0 {
            return Err(Refusal::Invalid);
        }
        Ok(())
    }

    /// Adds `len` bytes Expand set aside.
    pub const fn expand(&mut self, len: u64) {
        self.capacity = self.capacity.saturating_add(len);
    }

    /// Bytes set aside and not committed.
    #[must_use]
    pub const fn free(&self) -> u64 {
        self.capacity.saturating_sub(self.committed)
    }

    /// Records a reservation at `base`.
    pub fn reserve(&mut self, base: u64, len: u64) {
        self.reservations.insert(
            base,
            Reservation {
                len,
                committed: BTreeSet::new(),
            },
        );
    }

    /// Empties the reservation that is exactly `[base, base + len)`, for a fixed reserve landing on
    /// it, and answers the granules that were committed there. `None` when no reservation is
    /// exactly that range.
    pub fn replace(&mut self, base: u64, len: u64) -> Option<Vec<u64>> {
        let held = self.reservations.get_mut(&base).filter(|r| r.len == len)?;
        let dropped: Vec<u64> = std::mem::take(&mut held.committed).into_iter().collect();
        self.committed = self
            .committed
            .saturating_sub(dropped.len() as u64 * GRANULE);
        Some(dropped)
    }

    /// The reservation wholly holding `[addr, addr + len)`.
    fn holder(&mut self, addr: u64, len: u64) -> Option<&mut Reservation> {
        let end = addr.checked_add(len)?;
        let (&base, held) = self.reservations.range_mut(..=addr).next_back()?;
        (end <= base.saturating_add(held.len)).then_some(held)
    }

    /// Commits `[addr, addr + len)` and answers the granules newly backed.
    ///
    /// # Errors
    ///
    /// [`Refusal::Invalid`] for a range off the granule grid. [`Refusal::NoMemory`] when the pool
    /// has too little free. [`Refusal::Unmeasured`] for no bytes, a range outside every
    /// reservation, or a granule already committed.
    pub fn commit(&mut self, addr: u64, len: u64) -> Result<Vec<u64>, Refusal> {
        if len == 0 {
            return Err(Refusal::Unmeasured("a commit of no bytes"));
        }
        if !granular(addr, len) {
            return Err(Refusal::Invalid);
        }
        let free = self.free();
        let Some(held) = self.holder(addr, len) else {
            return Err(Refusal::Unmeasured(
                "a commit outside every pool reservation",
            ));
        };
        if granules(addr, len).any(|g| held.committed.contains(&g)) {
            return Err(Refusal::Unmeasured(
                "a commit over a granule already committed",
            ));
        }
        if len > free {
            return Err(Refusal::NoMemory);
        }
        held.committed.extend(granules(addr, len));
        self.committed = self.committed.saturating_add(len);
        Ok(granules(addr, len).collect())
    }

    /// Decommits `[addr, addr + len)` and answers the granules that were committed there. A
    /// granule not committed is passed over, as a second decommit answers 0.
    ///
    /// # Errors
    ///
    /// [`Refusal::Invalid`] for less than a granule or a range off the grid.
    /// [`Refusal::Unmeasured`] for a range outside every reservation.
    pub fn decommit(&mut self, addr: u64, len: u64) -> Result<Vec<u64>, Refusal> {
        if len < GRANULE || !granular(addr, len) {
            return Err(Refusal::Invalid);
        }
        let Some(held) = self.holder(addr, len) else {
            return Err(Refusal::Unmeasured(
                "a decommit outside every pool reservation",
            ));
        };
        let dropped: Vec<u64> = granules(addr, len)
            .filter(|g| held.committed.remove(g))
            .collect();
        self.committed = self
            .committed
            .saturating_sub(dropped.len() as u64 * GRANULE);
        Ok(dropped)
    }
}

/// The process's one pool.
fn pool() -> &'static Mutex<Pool> {
    static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();
    POOL.get_or_init(Mutex::default)
}

/// A status in the vendor encoding.
fn vendor(errno: u32) -> u64 {
    u64::from(GuestError::vendor(errno).as_raw())
}

/// The console faults writing through a null out-pointer (km01 notes -7389's `0xfffffff5` rows are
/// the probe guard's `-SIGSEGV`). A handler cannot fault the guest, so it answers what FreeBSD's
/// `copyout` answers for an address it cannot write: `EFAULT`.
fn faulted() -> u64 {
    vendor(errno::FAULT)
}

/// The status for `refusal`, logging an unmeasured one by the call's name.
fn refused(call: &str, refusal: Refusal) -> u64 {
    match refusal {
        Refusal::Invalid => vendor(errno::INVALID),
        Refusal::NoMemory => vendor(errno::NO_MEMORY),
        Refusal::Unmeasured(arm) => {
            tracing::warn!(
                "{call} refused: {arm} is not measured (obSCEne REQ-20261020T1310Z-km01 asks)"
            );
            u64::from(GuestError::Unimplemented.as_raw())
        }
    }
}

/// No access: a reserved granule nothing is committed in, which faults when read.
const NO_ACCESS: Protection = Protection {
    read: false,
    write: false,
    execute: false,
};

/// `sceKernelMemoryPoolExpand(search_start, search_end, len, align, phys_out)`: sets direct memory
/// aside for the pool, from `[search_start, search_end)`, and writes where it starts.
pub fn expand(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    const CALL: &str = "sceKernelMemoryPoolExpand";
    let (search_start, search_end, len, align, out) = (args[0], args[1], args[2], args[3], args[4]);
    if let Err(refusal) = Pool::check_expand(len, align) {
        return refused(CALL, refusal);
    }
    // Measured: a length of 0 answers 0.
    if len == 0 {
        return OK;
    }
    let start = {
        let Ok(mut map) = direct::map().lock() else {
            return u64::from(GuestError::Unimplemented.as_raw());
        };
        let Some(start) = map.allocate_aligned_from(search_start, len, align.max(GRANULE), 0)
        else {
            return refused(CALL, Refusal::NoMemory);
        };
        if start.checked_add(len).is_none_or(|end| end > search_end) {
            map.release(start, len);
            return refused(CALL, Refusal::NoMemory);
        }
        start
    };
    let Ok(mut pool) = pool().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    pool.expand(len);
    // Measured: a null out answers 0. The memory is set aside all the same; km01 1a measures the
    // budget either side of it.
    // SAFETY: a non-null address the guest passed for this call, valid by its contract.
    if out != 0 && !unsafe { guest::write_u64(out, start) } {
        return faulted();
    }
    OK
}

/// `sceKernelMemoryPoolReserve(addr, len, align, flags, addr_out)`: reserves address space for the
/// pool to commit into, with no access until a granule is committed.
///
/// `addr` is a hint, as an `mmap` address is: a held hint moves up to the first free range above
/// it, and `MAP_FIXED` takes the address exactly, emptying a pool reservation already there.
pub fn reserve(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    const CALL: &str = "sceKernelMemoryPoolReserve";
    let (hint, len, align, flags, out) = (args[0], args[1], args[2], args[3], args[4]);
    if len == 0 {
        return vendor(errno::INVALID);
    }
    if out == 0 {
        return faulted();
    }
    if flags & !MAP_FIXED != 0 {
        return refused(
            CALL,
            Refusal::Unmeasured("a reserve flag other than MAP_FIXED"),
        );
    }
    if align != 0 && !align.is_power_of_two() {
        return refused(
            CALL,
            Refusal::Unmeasured("a reserve alignment that is not a power of two"),
        );
    }
    let align = align.max(GRANULE).max(slot_unit());
    let Some(len) = checked_next_multiple_of(len, GRANULE) else {
        return vendor(errno::INVALID);
    };
    let fixed = flags & MAP_FIXED != 0;

    let Ok(mut space) = mappings().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let Ok(mut pool) = pool().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let base = if fixed {
        if hint % align != 0 {
            return refused(
                CALL,
                Refusal::Unmeasured("a fixed address off its alignment"),
            );
        }
        if let Some(dropped) = pool.replace(hint, len) {
            // The range stays reserved; what was committed in it goes.
            for granule in dropped {
                if space.protect(granule, GRANULE, NO_ACCESS).is_err() {
                    return vendor(errno::INVALID);
                }
            }
            drop(pool);
            drop(space);
            note_requested_protection(hint, len, 0);
            // SAFETY: a non-null address the guest passed for this call, valid by its contract.
            return if unsafe { guest::write_u64(out, hint) } {
                OK
            } else {
                faulted()
            };
        }
        if space.reserve(hint, len, NO_ACCESS).is_err() {
            return refused(
                CALL,
                Refusal::Unmeasured("a fixed address over memory that is not a pool reservation"),
            );
        }
        hint
    } else {
        let mut placed = None;
        if hint != 0
            && let Some(free) = space.first_free_at_or_above(hint, len, align)
            && space.reserve(free, len, NO_ACCESS).is_ok()
        {
            placed = Some(free);
        }
        for _ in 0..CONFLICT_RETRIES {
            if placed.is_some() {
                break;
            }
            let Some(next) = checked_next_multiple_of(next_mapping_base(len, align), align) else {
                break;
            };
            if space.reserve(next, len, NO_ACCESS).is_ok() {
                placed = Some(next);
            }
        }
        let Some(base) = placed else {
            return refused(CALL, Refusal::NoMemory);
        };
        base
    };
    pool.reserve(base, len);
    drop(pool);
    drop(space);
    mapping_placed(base, len, NO_ACCESS, hint != 0);
    note_requested_protection(base, len, 0);
    // SAFETY: a non-null address the guest passed for this call, valid by its contract.
    if unsafe { guest::write_u64(out, base) } {
        OK
    } else {
        faulted()
    }
}

/// Zeros, one granule's worth, for clearing a granule as it is committed.
static ZERO_GRANULE: [u8; GRANULE as usize] = [0; GRANULE as usize];

/// `sceKernelMemoryPoolCommit(addr, len, type, prot, flags)`: backs `[addr, addr + len)` from the
/// pool, at `prot`.
///
/// A newly committed granule reads as zero, as a fresh anonymous page does in FreeBSD
/// (`vm_fault`'s zero fill); km01 1d asks what the console holds after a recommit. The memory
/// type is not read.
pub fn commit(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    const CALL: &str = "sceKernelMemoryPoolCommit";
    let (addr, len, prot, flags) = (args[0], args[1], args[3], args[4]);
    if flags != 0 {
        return refused(CALL, Refusal::Unmeasured("a commit with flags"));
    }
    // The address space before the pool, in every handler, so two guest threads cannot each hold
    // the lock the other waits on.
    let Ok(mut space) = mappings().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let Ok(mut pool) = pool().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let committed = match pool.commit(addr, len) {
        Ok(granules) => granules,
        Err(refusal) => return refused(CALL, refusal),
    };
    let protection = protection_from_guest(prot);
    let backed = space.protect(addr, len, Protection::READ_WRITE).is_ok()
        && committed.iter().all(|&granule| {
            // SAFETY: the granule lies in a pool reservation this crate's address space holds,
            // made writable by the line above.
            unsafe { guest::write_bytes(granule, &ZERO_GRANULE) }
        })
        && space.protect(addr, len, protection).is_ok();
    if !backed {
        // Handed back, so the pool's accounting matches what is mapped.
        let _ = pool.decommit(addr, len);
        return vendor(errno::INVALID);
    }
    drop(pool);
    drop(space);
    note_requested_protection(addr, len, prot);
    if protection.read {
        orbistoun_thunk::note_readable_range(addr, len);
    }
    OK
}

/// `sceKernelMemoryPoolDecommit(addr, len, flags)`: returns the granules of `[addr, addr + len)`
/// to the pool, leaving the address space reserved and without access.
pub fn decommit(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    const CALL: &str = "sceKernelMemoryPoolDecommit";
    let (addr, len, flags) = (args[0], args[1], args[2]);
    if flags != 0 {
        return refused(CALL, Refusal::Unmeasured("a decommit with flags"));
    }
    let Ok(mut space) = mappings().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let Ok(mut pool) = pool().lock() else {
        return u64::from(GuestError::Unimplemented.as_raw());
    };
    let dropped = match pool.decommit(addr, len) {
        Ok(granules) => granules,
        Err(refusal) => return refused(CALL, refusal),
    };
    drop(pool);
    for granule in dropped {
        if space.protect(granule, GRANULE, NO_ACCESS).is_err() {
            return vendor(errno::INVALID);
        }
    }
    drop(space);
    note_requested_protection(addr, len, 0);
    OK
}

#[cfg(test)]
mod tests {
    use super::{GRANULE, Pool, Refusal};
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// `EINVAL` in the vendor encoding, as -7389's rows print it.
    const EINVAL: u64 = 0x8002_0016;

    fn args(values: [u64; 5]) -> [u64; GUEST_ARG_REGISTERS] {
        let mut out = [0; GUEST_ARG_REGISTERS];
        out[..5].copy_from_slice(&values);
        out
    }

    /// The Expand sweep as measured: 4 MiB at 64 KiB, length 0 and alignment 0 are accepted;
    /// length `0x1000` and alignment `0x3000` are `EINVAL`.
    #[test]
    fn expand_accepts_and_refuses_what_the_console_does() {
        assert_eq!(Pool::check_expand(0x40_0000, 0x1_0000), Ok(()));
        assert_eq!(Pool::check_expand(0, 0x1_0000), Ok(()));
        assert_eq!(Pool::check_expand(0x40_0000, 0), Ok(()));
        assert_eq!(Pool::check_expand(0x1000, 0x1_0000), Err(Refusal::Invalid));
        assert_eq!(Pool::check_expand(0x40_0000, 0x3000), Err(Refusal::Invalid));
        assert!(matches!(
            Pool::check_expand(0x40_0000, 0x4000),
            Err(Refusal::Unmeasured(_))
        ));
    }

    /// Commit charges the pool and decommit credits it; a granule decommitted twice is passed
    /// over, and less than a granule is `EINVAL`.
    #[test]
    fn commit_and_decommit_account_in_granules() {
        let mut pool = Pool::default();
        pool.expand(4 * GRANULE);
        let base = 0x10_0000_0000;
        pool.reserve(base, 0x40_0000);
        assert_eq!(pool.commit(base, GRANULE), Ok(vec![base]));
        assert_eq!(pool.free(), 3 * GRANULE);
        assert_eq!(
            pool.decommit(base + 0x2_0000, 0x1000),
            Err(Refusal::Invalid)
        );
        assert_eq!(pool.decommit(base, GRANULE), Ok(vec![base]));
        assert_eq!(pool.decommit(base, GRANULE), Ok(vec![]), "twice answers 0");
        assert_eq!(pool.free(), 4 * GRANULE);
    }

    /// A commit past what Expand set aside is out of memory, and nothing is charged for it.
    #[test]
    fn a_commit_past_the_pool_is_refused_whole() {
        let mut pool = Pool::default();
        pool.expand(GRANULE);
        let base = 0x20_0000_0000;
        pool.reserve(base, 0x40_0000);
        assert_eq!(pool.commit(base, 2 * GRANULE), Err(Refusal::NoMemory));
        assert_eq!(pool.free(), GRANULE);
    }

    /// The arms no row settles are refused by name, not answered.
    #[test]
    fn unmeasured_commits_refuse_by_name() {
        let mut pool = Pool::default();
        pool.expand(4 * GRANULE);
        let base = 0x30_0000_0000;
        pool.reserve(base, 0x40_0000);
        assert!(matches!(pool.commit(base, 0), Err(Refusal::Unmeasured(_))));
        assert!(matches!(
            pool.commit(base + 0x40_0000, GRANULE),
            Err(Refusal::Unmeasured(_))
        ));
        assert_eq!(pool.commit(base, GRANULE), Ok(vec![base]));
        assert!(matches!(
            pool.commit(base, GRANULE),
            Err(Refusal::Unmeasured(_))
        ));
        assert_eq!(pool.commit(base + 0x1000, GRANULE), Err(Refusal::Invalid));
    }

    /// A fixed reserve over a pool reservation empties it and hands its committed bytes back.
    #[test]
    fn a_fixed_reserve_over_a_reservation_empties_it() {
        let mut pool = Pool::default();
        pool.expand(2 * GRANULE);
        let base = 0x40_0000_0000;
        pool.reserve(base, 0x40_0000);
        assert_eq!(pool.commit(base, 2 * GRANULE).map(|g| g.len()), Ok(2));
        assert_eq!(pool.replace(base, 0x40_0000).map(|g| g.len()), Some(2));
        assert_eq!(pool.free(), 2 * GRANULE);
        assert_eq!(pool.replace(base, 0x20_0000), None, "only an exact range");
    }

    /// The handlers end to end, in the measured order: expand, reserve, commit, write and read a
    /// byte, query the protection, decommit, decommit a sub-granule and decommit twice.
    #[test]
    fn the_measured_sequence_runs_through_the_handlers() {
        let mut phys: u64 = 0xcccc_cccc_cccc_cccc;
        let phys_at = std::ptr::from_mut(&mut phys) as usize as u64;
        assert_eq!(
            super::expand(&args([0, 0x4_0000_0000, 0x40_0000, 0x1_0000, phys_at])),
            0
        );
        assert_eq!(phys % 0x1_0000, 0, "a granule-aligned physical address");
        assert_ne!(phys, 0xcccc_cccc_cccc_cccc, "written");
        assert_eq!(
            super::expand(&args([0, 0x4_0000_0000, 0, 0x1_0000, phys_at])),
            0
        );
        assert_eq!(
            super::expand(&args([0, 0x4_0000_0000, 0x1000, 0x1_0000, phys_at])),
            EINVAL
        );
        assert_eq!(
            super::expand(&args([0, 0x4_0000_0000, 0x40_0000, 0x3000, phys_at])),
            EINVAL
        );

        let mut addr: u64 = 0xcccc_cccc_cccc_cccc;
        let addr_at = std::ptr::from_mut(&mut addr) as usize as u64;
        assert_eq!(
            super::reserve(&args([0, 0x40_0000, 0x40_0000, 0, addr_at])),
            0
        );
        assert_eq!(addr % 0x40_0000, 0, "at the alignment asked for");
        let mut again: u64 = 0;
        let again_at = std::ptr::from_mut(&mut again) as usize as u64;
        assert_eq!(
            super::reserve(&args([addr, 0x40_0000, 0x40_0000, 0x10, again_at])),
            0,
            "fixed over a held pool reservation"
        );
        assert_eq!(again, addr);
        assert_eq!(
            super::reserve(&args([0, 0, 0x40_0000, 0, again_at])),
            EINVAL
        );

        assert_eq!(super::commit(&args([addr, 0x1_0000, 0, 0x33, 0])), 0);
        // SAFETY: the granule at `addr` was just committed read-write.
        assert!(unsafe { orbistoun_mem::guest::write_bytes(addr, &[0x5a]) });
        // SAFETY: as above.
        let read = unsafe { orbistoun_mem::guest::read_u32(addr) };
        assert_eq!(read.map(|v| v & 0xff), Some(0x5a));
        let (mut start, mut end, mut prot) = (0_u64, 0_u64, 0_u32);
        let at = |p: *mut u64| p as usize as u64;
        assert_eq!(
            super::super::query_memory_protection(&[
                addr,
                at(&raw mut start),
                at(&raw mut end),
                std::ptr::from_mut(&mut prot) as usize as u64,
                0,
                0
            ]),
            0
        );
        assert_eq!(
            (start, end, prot),
            (addr, addr + 0x40_0000, 0x33),
            "the reservation's bounds, at the protection committed"
        );

        assert_eq!(super::decommit(&args([addr, 0x1_0000, 0, 0, 0])), 0);
        assert_eq!(
            super::decommit(&args([addr + 0x2_0000, 0x1000, 0, 0, 0])),
            EINVAL
        );
        assert_eq!(super::decommit(&args([addr, 0x1_0000, 0, 0, 0])), 0);

        // Recommitted, the granule reads as fresh memory.
        assert_eq!(super::commit(&args([addr, 0x1_0000, 0, 0x33, 0])), 0);
        // SAFETY: committed read-write by the line above.
        assert_eq!(unsafe { orbistoun_mem::guest::read_u64(addr) }, Some(0));
        assert_eq!(super::decommit(&args([addr, 0x1_0000, 0, 0, 0])), 0);
    }

    /// A null out is where the console faults; it is answered `EFAULT`, nothing reserved.
    #[test]
    fn a_reserve_with_nowhere_to_write_is_a_fault() {
        assert_eq!(
            super::reserve(&args([0, 0x40_0000, 0x40_0000, 0, 0])),
            0x8002_000e
        );
    }
}
