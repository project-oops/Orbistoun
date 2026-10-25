//! Guest memory committed as the guest touches it.
//!
//! The console backs a mapping as its pages are used; a host that commits every mapping whole
//! charges the process for gigabytes no page of which is ever read. Once [`commit_lazily`] is
//! called - by whoever installs the fault handler that asks [`commit_on_touch`] - a reservation
//! takes address space only, and its pages are committed on first touch, a chunk at a time, with
//! the protection the guest last set for them. A protection set over pages not yet committed is
//! recorded and applied when they are.
//!
//! Two kinds of access cannot wait for a fault. The host's kernel writing guest memory - a file
//! read into a guest buffer - reaches no handler, so such a write commits its range first
//! ([`commit`], through [`crate::watch::release_overlapping`]). And a stack must be committed
//! before code runs on it: the fault that would commit a stack page is delivered on that same
//! stack ([`commit`] from [`crate::stack::GuestStack::reserve`] and from whoever hands a thread
//! or fiber a stack of the guest's own).

use std::collections::BTreeMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::Protection;

/// How much a touch commits at most: the aligned chunk around it, so a guest walking a buffer
/// faults once per chunk rather than once per page.
pub const TOUCH_CHUNK: u64 = 64 << 10;

/// Whether reservations made from now on are committed lazily.
static LAZILY: AtomicBool = AtomicBool::new(false);

/// Bytes reserved and committed at once, outside this module's care: before [`commit_lazily`],
/// or off Windows, where the host commits on touch itself.
static EAGER: AtomicU64 = AtomicU64::new(0);

/// Bytes reserved to commit on touch, and of those bytes committed: every reservation's, counted
/// as they change so [`census`] reads them without the registry's lock.
static LAZY_RESERVED: AtomicU64 = AtomicU64::new(0);
static LAZY_COMMITTED: AtomicU64 = AtomicU64::new(0);

/// Takes `bytes` from `counter`, never below nothing.
fn take(counter: &AtomicU64, bytes: u64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |held| {
        Some(held.saturating_sub(bytes))
    });
}

/// Makes every later reservation commit on touch. Called once the fault handler that asks
/// [`commit_on_touch`] is installed, and never before: a touch would otherwise end the process.
pub fn commit_lazily() {
    LAZILY.store(cfg!(windows), Ordering::Release);
}

pub(crate) fn lazily() -> bool {
    LAZILY.load(Ordering::Acquire)
}

/// Notes `len` bytes reserved and committed whole.
pub(crate) fn note_eager(len: u64) {
    EAGER.fetch_add(len, Ordering::Relaxed);
}

/// Notes `len` bytes committed whole given back.
pub(crate) fn note_eager_released(len: u64) {
    EAGER.fetch_sub(len.min(EAGER.load(Ordering::Relaxed)), Ordering::Relaxed);
}

/// What the guest's memory holds of the host's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuestCommit {
    /// Bytes reserved to commit on touch.
    pub lazy_reserved: u64,
    /// Of those, bytes committed.
    pub lazy_committed: u64,
    /// Bytes reserved and committed whole.
    pub eager: u64,
}

impl GuestCommit {
    /// Every byte of guest memory the host has committed.
    #[must_use]
    pub const fn committed(&self) -> u64 {
        self.lazy_committed + self.eager
    }
}

/// What the guest's memory holds of the host's now. Takes no lock: a census asks it from any
/// thread, which may hold anything.
#[must_use]
pub fn census() -> GuestCommit {
    GuestCommit {
        lazy_reserved: LAZY_RESERVED.load(Ordering::Relaxed),
        lazy_committed: LAZY_COMMITTED.load(Ordering::Relaxed),
        eager: EAGER.load(Ordering::Relaxed),
    }
}

/// One reservation committed on touch.
#[derive(Debug)]
struct Lazy {
    len: u64,
    protections: Protections,
    /// Bytes of it committed.
    committed: AtomicU64,
}

fn registry() -> &'static RwLock<BTreeMap<u64, Lazy>> {
    static REGISTRY: RwLock<BTreeMap<u64, Lazy>> = RwLock::new(BTreeMap::new());
    &REGISTRY
}

/// Records a reservation of `len` bytes at `base`, its pages `protection` until set otherwise.
pub(crate) fn register(base: u64, len: u64, protection: Protection) {
    let mut held = registry()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    LAZY_RESERVED.fetch_add(len, Ordering::Relaxed);
    let replaced = held.insert(
        base,
        Lazy {
            len,
            protections: Protections::new(base, base.saturating_add(len), protection),
            committed: AtomicU64::new(0),
        },
    );
    if let Some(old) = replaced {
        uncount(&old);
    }
}

/// Takes what `lazy` counted out of the counts [`census`] reads, as it is forgotten.
fn uncount(lazy: &Lazy) {
    take(&LAZY_RESERVED, lazy.len);
    take(&LAZY_COMMITTED, lazy.committed.load(Ordering::Relaxed));
}

/// Forgets the reservation at `base`, released; whether it was one.
pub(crate) fn unregister(base: u64) -> bool {
    let removed = registry()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&base);
    removed.as_ref().inspect(|lazy| uncount(lazy)).is_some()
}

/// The reservation holding `address`, as its base and itself.
fn holding(held: &BTreeMap<u64, Lazy>, address: u64) -> Option<(u64, &Lazy)> {
    let (&base, lazy) = held.range(..=address).next_back()?;
    (address < base.saturating_add(lazy.len)).then_some((base, lazy))
}

/// Commits every page of `[base, base + len)` lying in a reservation committed on touch that is
/// not committed yet, with the protection recorded for it: before the host's kernel writes the
/// range, or code runs on it as a stack. Pages the guest may not access are left as they are.
pub fn commit(base: u64, len: u64) {
    let end = base.saturating_add(len);
    let held = registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (&start, lazy) in held.range(..end) {
        let (from, to) = (base.max(start), end.min(start.saturating_add(lazy.len)));
        if from < to {
            commit_in(lazy, from, to);
        }
    }
}

/// The parts of `[base, base + len)` whose host pages are committed, private, and readable and
/// writable, as (base, length), in order: what a device may import and store through (D964). A
/// page left to commit on touch, one a guard holds read-only, and one the guest may not write are
/// not among them, since a driver pins and writes every page it imports.
#[must_use]
pub fn writable_runs(base: u64, len: u64) -> Vec<(u64, u64)> {
    imp::writable_runs(base, base.saturating_add(len))
}

/// The parts of `[base, base + len)` whose host pages are committed, private and readable, a guard
/// or the guest's own protection holding them read-only or not, as (base, length), in order: the
/// pages a device's mapping left out that may have become its to map since (D968).
#[must_use]
pub fn committed_runs(base: u64, len: u64) -> Vec<(u64, u64)> {
    imp::committed_runs(base, base.saturating_add(len))
}

/// Commits the chunk around `address` when it lies in a reservation committed on touch, is not
/// committed, and the guest may access it: what the fault handler asks of a faulting access
/// before anything else. Whether it did, so the access runs again.
pub fn commit_on_touch(address: u64) -> bool {
    let held = registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some((base, lazy)) = holding(&held, address) else {
        return false;
    };
    let Some((run_start, run_end, protection)) = lazy.protections.run_at(address) else {
        return false;
    };
    if protection == Protection::NONE || !imp::uncommitted(address) {
        return false;
    }
    let chunk = address & !(TOUCH_CHUNK - 1);
    let from = chunk.max(run_start).max(base);
    let to = chunk
        .saturating_add(TOUCH_CHUNK)
        .min(run_end)
        .min(base.saturating_add(lazy.len));
    commit_in(lazy, from, to) > 0
}

/// Commits the uncommitted pages of `[from, to)`, inside `lazy`, by its recorded protections;
/// the bytes committed.
fn commit_in(lazy: &Lazy, from: u64, to: u64) -> u64 {
    let mut committed = 0;
    for (start, end, protection) in lazy.protections.runs(from, to) {
        if protection != Protection::NONE {
            committed += imp::commit_uncommitted(start, end, protection);
        }
    }
    lazy.committed.fetch_add(committed, Ordering::Relaxed);
    LAZY_COMMITTED.fetch_add(committed, Ordering::Relaxed);
    if committed > 0 {
        // A page committed now is one the device may map, though no readable run changed (D968).
        crate::mappings::pages_changed();
    }
    committed
}

/// Sets `protection` over `[base, base + len)` where it lies in reservations committed on touch:
/// recorded for pages not committed yet, applied to those that are. `None` when no part of it lies
/// in one, and the caller protects it as any committed range.
pub(crate) fn protect(base: u64, len: u64, protection: Protection) -> Option<Result<(), String>> {
    let end = base.saturating_add(len);
    let mut held = registry()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut any = false;
    for (&start, lazy) in held.range_mut(..end) {
        let (from, to) = (base.max(start), end.min(start.saturating_add(lazy.len)));
        if from < to {
            lazy.protections.set(from, to, protection);
            any = true;
        }
    }
    any.then(|| imp::protect_committed(base, end, protection))
}

/// The protection of each page of a reservation, as runs: `(start, (end, protection))`, covering
/// it whole without overlap.
#[derive(Debug)]
struct Protections(BTreeMap<u64, (u64, Protection)>);

impl Protections {
    fn new(start: u64, end: u64, protection: Protection) -> Self {
        Self(BTreeMap::from([(start, (end, protection))]))
    }

    /// The run holding `address`.
    fn run_at(&self, address: u64) -> Option<(u64, u64, Protection)> {
        let (&start, &(end, protection)) = self.0.range(..=address).next_back()?;
        (address < end).then_some((start, end, protection))
    }

    /// The runs over `[from, to)`, cut to it.
    fn runs(&self, from: u64, to: u64) -> Vec<(u64, u64, Protection)> {
        let first = self.run_at(from).map_or(from, |(start, _, _)| start);
        self.0
            .range(first..to)
            .map(|(&start, &(end, protection))| (start.max(from), end.min(to), protection))
            .filter(|(start, end, _)| start < end)
            .collect()
    }

    /// Gives `[from, to)` `protection`, splitting the runs it cuts and joining equal neighbours.
    fn set(&mut self, from: u64, to: u64, protection: Protection) {
        self.split_at(from);
        self.split_at(to);
        let inside: Vec<u64> = self.0.range(from..to).map(|(&start, _)| start).collect();
        for start in inside {
            self.0.remove(&start);
        }
        self.0.insert(from, (to, protection));
        self.join_at(to);
        self.join_at(from);
    }

    /// Splits the run holding `at` so one starts there.
    fn split_at(&mut self, at: u64) {
        if let Some((start, end, protection)) = self.run_at(at)
            && start < at
        {
            self.0.insert(start, (at, protection));
            self.0.insert(at, (end, protection));
        }
    }

    /// Joins the run starting at `at` onto the one ending there, when their protections agree.
    fn join_at(&mut self, at: u64) {
        let Some(&(end, protection)) = self.0.get(&at) else {
            return;
        };
        if let Some((&before, &(before_end, before_protection))) = self.0.range(..at).next_back()
            && before_end == at
            && before_protection == protection
        {
            self.0.remove(&at);
            self.0.insert(before, (end, protection));
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_PRIVATE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_READONLY, PAGE_READWRITE, VirtualAlloc, VirtualProtect,
        VirtualQuery,
    };

    use crate::Protection;

    /// The host's state of the pages from `at`: where their run of alike pages ends, and its
    /// state. `None` when it says nothing.
    fn query(at: u64) -> Option<(u64, u32)> {
        query_info(at).map(|(run_end, info)| (run_end, info.State))
    }

    /// The host's description of the pages from `at`: where their run of alike pages ends, and
    /// the description. `None` when it says nothing.
    fn query_info(at: u64) -> Option<(u64, MEMORY_BASIC_INFORMATION)> {
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
        // call, and nothing at `at` is read.
        let answered = unsafe {
            VirtualQuery(
                std::ptr::with_exposed_provenance::<core::ffi::c_void>(usize::try_from(at).ok()?),
                &raw mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        let run_end = (info.BaseAddress as u64).saturating_add(info.RegionSize as u64);
        (answered != 0 && run_end > at).then_some((run_end, info))
    }

    /// The runs of `[from, to)` that are committed private memory, readable and writable with no
    /// modifier bits, joined where they touch.
    pub(super) fn writable_runs(from: u64, to: u64) -> Vec<(u64, u64)> {
        // The whole protection word, so a guard, no-cache or write-combine bit refuses it too.
        runs_where(from, to, |info| {
            info.State == MEM_COMMIT
                && info.Type == MEM_PRIVATE
                && (info.Protect == PAGE_READWRITE || info.Protect == PAGE_EXECUTE_READWRITE)
        })
    }

    /// The runs of `[from, to)` that are committed private memory the guest may read, whatever
    /// else holds them read-only, joined where they touch.
    pub(super) fn committed_runs(from: u64, to: u64) -> Vec<(u64, u64)> {
        runs_where(from, to, |info| {
            info.State == MEM_COMMIT
                && info.Type == MEM_PRIVATE
                && matches!(
                    info.Protect & 0xff,
                    PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE
                )
        })
    }

    /// The runs of `[from, to)` whose host description `keep` admits, joined where they touch.
    fn runs_where(
        from: u64,
        to: u64,
        keep: impl Fn(&MEMORY_BASIC_INFORMATION) -> bool,
    ) -> Vec<(u64, u64)> {
        let mut runs: Vec<(u64, u64)> = Vec::new();
        let mut at = from;
        while at < to {
            let Some((run_end, info)) = query_info(at) else {
                break;
            };
            let piece_end = run_end.min(to);
            if keep(&info) {
                match runs.last_mut() {
                    Some((base, len)) if *base + *len == at => *len += piece_end - at,
                    _ => runs.push((at, piece_end - at)),
                }
            }
            at = piece_end;
        }
        runs
    }

    /// Whether the page holding `address` is reserved and not committed.
    pub(super) fn uncommitted(address: u64) -> bool {
        query(address).is_some_and(|(_, state)| state == MEM_RESERVE)
    }

    /// Commits the pages of `[from, to)` that are reserved and not committed with `protection`,
    /// leaving committed ones as they are - a guard's protection among them; the bytes committed.
    pub(super) fn commit_uncommitted(from: u64, to: u64, protection: Protection) -> u64 {
        let mut at = from;
        let mut committed = 0;
        while at < to {
            let Some((run_end, state)) = query(at) else {
                break;
            };
            let piece_end = run_end.min(to);
            if state == MEM_RESERVE
                && let (Ok(address), Ok(size)) =
                    (usize::try_from(at), usize::try_from(piece_end - at))
            {
                // SAFETY: `[at, piece_end)` lies in a reservation this process made and still
                // holds, and is reserved, not committed: committing it changes no byte anything
                // holds, and a commit that fails returns null and changes nothing.
                let got = unsafe {
                    VirtualAlloc(
                        std::ptr::with_exposed_provenance::<core::ffi::c_void>(address),
                        size,
                        MEM_COMMIT,
                        crate::platform::protection_flags(protection),
                    )
                };
                if !got.is_null() {
                    committed += piece_end - at;
                }
            }
            at = piece_end;
        }
        committed
    }

    /// Applies `protection` to the committed pages of `[from, to)`; the reserved ones carry it in
    /// the record until they are committed.
    pub(super) fn protect_committed(
        from: u64,
        to: u64,
        protection: Protection,
    ) -> Result<(), String> {
        let mut at = from;
        while at < to {
            let (run_end, state) =
                query(at).ok_or_else(|| format!("nothing is mapped at {at:#x}"))?;
            let piece_end = run_end.min(to);
            if state == MEM_COMMIT {
                let (Ok(address), Ok(size)) =
                    (usize::try_from(at), usize::try_from(piece_end - at))
                else {
                    return Err(format!("{at:#x} does not fit a pointer"));
                };
                let mut old = 0u32;
                // SAFETY: `[at, piece_end)` is committed memory in a reservation this process
                // owns; changing its protection changes no bytes, and `old` is a live local.
                let ok = unsafe {
                    VirtualProtect(
                        std::ptr::with_exposed_provenance_mut::<core::ffi::c_void>(address),
                        size,
                        crate::platform::protection_flags(protection),
                        &raw mut old,
                    )
                };
                if ok == 0 {
                    return Err(format!("VirtualProtect refused {at:#x}..{piece_end:#x}"));
                }
            }
            at = piece_end;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::Protection;

    /// Off Windows nothing is reserved lazily here: the host commits on touch itself.
    pub(super) const fn uncommitted(_address: u64) -> bool {
        false
    }

    /// Off Windows the range is taken as it stands.
    pub(super) fn writable_runs(from: u64, to: u64) -> Vec<(u64, u64)> {
        if from < to {
            vec![(from, to - from)]
        } else {
            Vec::new()
        }
    }

    /// Off Windows the range is taken as it stands.
    pub(super) fn committed_runs(from: u64, to: u64) -> Vec<(u64, u64)> {
        writable_runs(from, to)
    }

    pub(super) const fn commit_uncommitted(_from: u64, _to: u64, _protection: Protection) -> u64 {
        0
    }

    pub(super) const fn protect_committed(
        _from: u64,
        _to: u64,
        _protection: Protection,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Protection, Protections};

    /// Setting a protection over part of a reservation splits its run, and setting it back joins
    /// the runs again.
    #[test]
    fn protections_split_and_join() {
        let mut runs = Protections::new(0x1000, 0x9000, Protection::READ_WRITE);
        runs.set(0x3000, 0x5000, Protection::NONE);
        assert_eq!(
            runs.runs(0x1000, 0x9000),
            vec![
                (0x1000, 0x3000, Protection::READ_WRITE),
                (0x3000, 0x5000, Protection::NONE),
                (0x5000, 0x9000, Protection::READ_WRITE),
            ]
        );
        assert_eq!(
            runs.run_at(0x4fff),
            Some((0x3000, 0x5000, Protection::NONE))
        );
        assert_eq!(
            runs.runs(0x2000, 0x4000),
            vec![
                (0x2000, 0x3000, Protection::READ_WRITE),
                (0x3000, 0x4000, Protection::NONE),
            ],
            "cut to the range asked"
        );
        runs.set(0x3000, 0x5000, Protection::READ_WRITE);
        assert_eq!(runs.0.len(), 1, "{:?}", runs.0);
        runs.set(0x1000, 0x2000, Protection::READ_ONLY);
        runs.set(0x8000, 0x9000, Protection::READ_ONLY);
        assert_eq!(runs.0.len(), 3);
        assert_eq!(runs.run_at(0x9000), None, "past its end");
    }

    /// A reservation is counted while it is registered and not once forgotten, and the census
    /// answers while another thread holds the registry.
    #[test]
    fn the_census_counts_reservations_without_the_registry() {
        use std::sync::mpsc::channel;
        const LEN: u64 = 1 << 40;
        let base = 0x7f00_0000_0000_0000;
        super::register(base, LEN, Protection::READ_WRITE);
        assert!(super::census().lazy_reserved >= LEN, "counted");
        let (locked, held) = channel();
        let (release, released) = channel::<()>();
        let holder = std::thread::spawn(move || {
            let registry = super::registry()
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            locked.send(()).expect("the test waits");
            let _ = released.recv_timeout(std::time::Duration::from_secs(30));
            drop(registry);
        });
        held.recv().expect("the holder locks");
        let (answer, answered) = channel();
        std::thread::spawn(move || {
            let _ = answer.send(super::census());
        });
        let census = answered.recv_timeout(std::time::Duration::from_secs(5));
        let _ = release.send(());
        holder.join().expect("the holder lets go");
        assert!(census.is_ok(), "the census waited on the registry");
        assert!(super::unregister(base));
        assert!(
            super::census().lazy_reserved < LEN,
            "and not once forgotten"
        );
    }

    /// A reservation committed on touch holds no memory until touched; a touch commits the chunk
    /// around it with the protection the guest set, a protected page is not committed by a touch,
    /// and a range committed for the host's kernel holds what it is given.
    #[cfg(windows)]
    #[test]
    fn guest_memory_is_committed_as_it_is_touched() {
        use super::{TOUCH_CHUNK, commit, commit_on_touch, imp};
        let base = crate::unique_test_base();
        let len = 4 * TOUCH_CHUNK;
        let held =
            crate::platform::reserve_lazily(base, len, Protection::READ_WRITE).expect("reserved");
        let committed = |from: u64| {
            super::registry().read().expect("registry")[&from]
                .committed
                .load(std::sync::atomic::Ordering::Relaxed)
        };
        assert_eq!(committed(base), 0);
        assert!(imp::uncommitted(base + 0x10));

        crate::platform::protect(base + 3 * TOUCH_CHUNK, TOUCH_CHUNK, Protection::NONE)
            .expect("recorded on pages not committed");
        assert!(
            !commit_on_touch(base + 3 * TOUCH_CHUNK + 8),
            "the guest may not touch it"
        );
        assert!(commit_on_touch(base + TOUCH_CHUNK + 8));
        assert!(
            !commit_on_touch(base + TOUCH_CHUNK + 8),
            "committed already"
        );
        assert_eq!(committed(base), TOUCH_CHUNK, "one chunk");
        assert!(imp::uncommitted(base), "nothing else");
        let at = std::ptr::with_exposed_provenance_mut::<u8>((base + TOUCH_CHUNK) as usize);
        // SAFETY: the chunk was just committed read-write in a reservation this test holds.
        unsafe { at.write_volatile(0x5a) };

        crate::platform::protect(base, TOUCH_CHUNK, Protection::READ_ONLY).expect("recorded");
        commit(base, 2 * TOUCH_CHUNK);
        assert_eq!(committed(base), 2 * TOUCH_CHUNK);
        assert!(
            imp::protect_committed(base, base + 1, Protection::READ_ONLY).is_ok()
                && !imp::uncommitted(base)
        );
        // SAFETY: committed read-only by the call above, in a reservation this test holds.
        let first =
            unsafe { std::ptr::with_exposed_provenance::<u8>(base as usize).read_volatile() };
        assert_eq!(first, 0, "committed zeroed");
        drop(held);
        assert!(!super::unregister(base), "forgotten once released");
    }

    /// Only committed, readable and writable pages are a device's to import: run 335 of Grand
    /// Theft Auto V crashed the driver importing a guest run whole.
    #[cfg(windows)]
    #[test]
    fn a_device_imports_only_committed_writable_pages() {
        use super::{TOUCH_CHUNK, commit_on_touch, writable_runs};
        let base = crate::unique_test_base();
        let len = 4 * TOUCH_CHUNK;
        let _held =
            crate::platform::reserve_lazily(base, len, Protection::READ_WRITE).expect("reserved");
        assert_eq!(writable_runs(base, len), vec![], "nothing committed");
        assert!(commit_on_touch(base + 8));
        assert!(commit_on_touch(base + TOUCH_CHUNK + 8));
        assert!(commit_on_touch(base + 3 * TOUCH_CHUNK + 8));
        assert_eq!(
            writable_runs(base, len),
            vec![
                (base, 2 * TOUCH_CHUNK),
                (base + 3 * TOUCH_CHUNK, TOUCH_CHUNK)
            ],
            "the uncommitted chunk splits them"
        );
        crate::platform::protect(base + TOUCH_CHUNK, TOUCH_CHUNK, Protection::READ_ONLY)
            .expect("protected");
        assert_eq!(
            writable_runs(base, len),
            vec![(base, TOUCH_CHUNK), (base + 3 * TOUCH_CHUNK, TOUCH_CHUNK)],
            "a read-only chunk is left out"
        );
        assert_eq!(
            writable_runs(base + 8, 0x10),
            vec![(base + 8, 0x10)],
            "cut to the range asked"
        );
    }

    /// A page committed on touch, or given another protection, after a device mapped its run moves
    /// the host pages' generation, which no readable run's change would show; and a committed page
    /// held read-only is among the committed runs a mapping looks at again (D968).
    #[cfg(windows)]
    #[test]
    fn a_commit_or_protection_moves_the_host_pages_generation() {
        use super::{TOUCH_CHUNK, commit_on_touch, committed_runs};
        use crate::mappings::pages_generation;
        let base = crate::unique_test_base();
        let len = 4 * TOUCH_CHUNK;
        let _held =
            crate::platform::reserve_lazily(base, len, Protection::READ_WRITE).expect("reserved");
        let before = pages_generation();
        assert!(commit_on_touch(base + 2 * TOUCH_CHUNK + 8));
        let committed = pages_generation();
        assert!(committed > before, "a commit moves it");
        assert_eq!(
            committed_runs(base, len),
            vec![(base + 2 * TOUCH_CHUNK, TOUCH_CHUNK)]
        );
        crate::platform::protect(base + 2 * TOUCH_CHUNK, TOUCH_CHUNK, Protection::READ_ONLY)
            .expect("protected");
        assert!(pages_generation() > committed, "a protection moves it");
        assert_eq!(
            committed_runs(base, len),
            vec![(base + 2 * TOUCH_CHUNK, TOUCH_CHUNK)],
            "a read-only page is still committed"
        );
    }
}
