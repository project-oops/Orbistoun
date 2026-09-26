//! Guest synchronisation primitives.
//!
//! The guest locks in one call and unlocks in another, with arbitrary guest code between, so no
//! host frame can hold a `MutexGuard` for the critical section. Each lock is a host mutex over a
//! small state plus a condition variable; the host mutex is held only while the state is
//! inspected. Ownership is tracked, so a recursive lock re-taken by its owner does not deadlock
//! and a non-recursive one is not silently granted.
//!
//! The guest holds the address of a zeroed block this crate owns, as with thread handles (see
//! `thread::ThreadHandle`). The block is never written, since the real layout is not known.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use crate::thread::{NO_THREAD, ThreadHandle};

/// How the guest refers to a lock.
pub type MutexHandle = u64;

/// Handle meaning "no lock", and what a lookup miss looks like.
pub const NO_MUTEX: MutexHandle = 0;

/// What a guest holds for a semaphore.
///
/// A mutex is a `void *` but a semaphore is an `int` written through an out-pointer: four bytes,
/// not eight, so it has its own type (D272).
pub type SemaphoreHandle = i32;

/// Sentinel for "no semaphore here".
pub const NO_SEMAPHORE: SemaphoreHandle = 0;

/// Whether a lock may be taken twice by the thread already holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Recursion {
    /// A second acquisition by the owner is an error.
    ///
    /// The default, as in POSIX, so a double-lock bug in the guest is not silent.
    #[default]
    Forbidden,
    /// The owner may acquire it repeatedly, and must release it as many times.
    Allowed,
    /// A second acquisition by the owner is reported as a deadlock, neither blocked nor allowed.
    ///
    /// The platform's error-checking mutex answers a self-`trylock` with a code distinct from busy.
    Errorcheck,
}

/// The three answers an acquisition can give.
///
/// The owner re-taking a `Forbidden` lock is busy and re-taking an `Errorcheck` one is a
/// deadlock, and the platform gives those two different codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acquisition {
    /// Taken - free, or a recursive re-take by the owner.
    Locked,
    /// Not taken: held by another thread, taken by the owner of a non-recursive lock, or, under
    /// [`Blocking::Until`], still held when the deadline passed.
    ///
    /// The caller knows which patience it asked for, so it knows whether this means busy or timed
    /// out and which error the guest is owed.
    Busy,
    /// The owner re-taking an error-checking lock, which is a deadlock it is told about.
    Deadlock,
}

/// How long an acquisition may wait for what it wants.
///
/// One parameter for locks, semaphores and read-write locks, which are the same operation at
/// different levels of patience.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocking {
    /// Take it if it is free this instant; refuse rather than wait.
    Never,
    /// Wait for as long as it takes.
    Forever,
    /// Wait until this moment, then give up.
    ///
    /// An instant rather than a span, so repeated wakes cannot restart it (see `wait_while`).
    Until(Instant),
}

/// Waits on `signal` until `blocked` stops holding of the guarded state, or patience runs out.
///
/// Answers `Some` holding the guard, with the predicate false; `None` when the wait gave up,
/// which is a refusal under [`Blocking::Never`], an expired deadline under
/// [`Blocking::Until`], and a poisoned host mutex under any of them.
///
/// The remaining span is recomputed from the fixed deadline each turn: every release here
/// notifies all waiters, and handing the whole span to each `wait_timeout` would restart the
/// clock on every empty wake. A deadline already past is a wait of no time, not an error, as
/// POSIX specifies for `pthread_mutex_timedlock` on a free mutex.
fn wait_while<'a, T>(
    signal: &Condvar,
    mut guard: MutexGuard<'a, T>,
    until: Blocking,
    blocked: impl Fn(&T) -> bool,
) -> Option<MutexGuard<'a, T>> {
    while blocked(&guard) {
        match until {
            Blocking::Never => return None,
            Blocking::Forever => guard = signal.wait(guard).ok()?,
            Blocking::Until(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let (next, outcome) = signal.wait_timeout(guard, remaining).ok()?;
                guard = next;
                // Give up only when still blocked and the clock, not the timeout flag, says the deadline has
                // passed. `wait_timeout` reports a timeout from the platform timer, which can be slightly
                // coarser than `Instant` and fire a fraction of a millisecond early.
                if outcome.timed_out() && blocked(&guard) && Instant::now() >= deadline {
                    return None;
                }
            }
        }
    }
    Some(guard)
}

/// Who holds a lock, and how many times.
#[derive(Debug, Default)]
struct Held {
    owner: ThreadHandle,
    depth: u32,
}

/// A lock the guest can hold across calls.
#[derive(Debug)]
struct GuestMutex {
    recursion: Recursion,
    state: Mutex<Held>,
    released: Condvar,
    name: String,
}

impl GuestMutex {
    fn new(recursion: Recursion, name: String) -> Self {
        Self {
            recursion,
            state: Mutex::new(Held::default()),
            released: Condvar::new(),
            name,
        }
    }

    /// Takes the lock, waiting as long as `until` allows.
    ///
    /// The self-relock check comes before any waiting: a non-recursive lock re-taken by its owner
    /// can never become free, so it is answered the same way however patient the caller is.
    fn acquire(&self, by: ThreadHandle, until: Blocking) -> Acquisition {
        let Ok(mut held) = self.state.lock() else {
            return Acquisition::Busy;
        };
        if held.depth > 0 && held.owner == by {
            return match self.recursion {
                Recursion::Allowed => {
                    held.depth += 1;
                    Acquisition::Locked
                }
                Recursion::Errorcheck => Acquisition::Deadlock,
                Recursion::Forbidden => Acquisition::Busy,
            };
        }
        let Some(mut held) = wait_while(&self.released, held, until, |h| h.depth > 0) else {
            return Acquisition::Busy;
        };
        held.owner = by;
        held.depth = 1;
        Acquisition::Locked
    }

    /// Releases the lock.
    ///
    /// Returns `false` when the caller does not hold it; releasing somebody else's lock would let
    /// two guest threads into one critical section.
    fn unlock(&self, by: ThreadHandle) -> bool {
        let Ok(mut held) = self.state.lock() else {
            return false;
        };
        if held.depth == 0 || held.owner != by {
            return false;
        }
        held.depth -= 1;
        if held.depth == 0 {
            held.owner = NO_THREAD;
            self.released.notify_one();
        }
        true
    }
}

/// A counting semaphore the guest holds across calls.
///
/// Same shape as [`GuestMutex`]: the guest creates it in one call and uses it in another. A
/// `Condvar` carries the waiters, and the count is plain because every operation on it happens
/// under the lock.
#[derive(Debug)]
struct GuestSemaphore {
    state: Mutex<u32>,
    available: Condvar,
    /// The most the count may reach, so a signal past the ceiling can be refused.
    ceiling: u32,
    name: String,
}

impl GuestSemaphore {
    fn new(initial: u32, ceiling: u32, name: String) -> Self {
        Self {
            state: Mutex::new(initial),
            available: Condvar::new(),
            ceiling,
            name,
        }
    }

    /// Takes `need` at once or none at all, waiting as long as `until` allows.
    ///
    /// All-or-nothing, as on the hardware: asking for two where one is left answers busy. A caller
    /// granted part of a request could neither tell nor give it back.
    fn take(&self, need: u32, until: Blocking) -> bool {
        let Ok(count) = self.state.lock() else {
            return false;
        };
        let Some(mut count) = wait_while(&self.available, count, until, |c| *c < need) else {
            return false;
        };
        *count -= need;
        true
    }

    /// How many are free right now.
    ///
    /// A snapshot that may be stale when returned, which is the standard's own contract for
    /// `sem_getvalue`.
    fn value(&self) -> Option<u32> {
        self.state.lock().ok().map(|count| *count)
    }

    /// Returns `n`, refusing to exceed the ceiling.
    ///
    /// Refused rather than clamped, so a guest that has lost count is told.
    fn signal(&self, n: u32) -> bool {
        let Ok(mut count) = self.state.lock() else {
            return false;
        };
        let Some(raised) = count.checked_add(n) else {
            return false;
        };
        if raised > self.ceiling {
            return false;
        }
        *count = raised;
        self.available.notify_all();
        true
    }
}

/// The next semaphore handle.
///
/// A counter, not a pointer: a 48-bit address truncated to a four-byte `int` could collide with
/// another semaphore's. Starts at one, so zero means "nothing here" for a field a guest zeroed.
fn next_semaphore_handle() -> SemaphoreHandle {
    static NEXT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn semaphores() -> &'static Mutex<BTreeMap<SemaphoreHandle, Arc<GuestSemaphore>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<SemaphoreHandle, Arc<GuestSemaphore>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Creates a semaphore and returns the handle the guest should hold.
pub fn create_semaphore(initial: u32, ceiling: u32, name: &str) -> SemaphoreHandle {
    let handle = next_semaphore_handle();
    if let Ok(mut table) = semaphores().lock() {
        table.insert(
            handle,
            Arc::new(GuestSemaphore::new(initial, ceiling, name.to_owned())),
        );
    }
    handle
}

/// Runs `f` against a semaphore, with the table released first.
///
/// Holding the table across a blocking wait would deadlock every thread that could signal it.
fn with_semaphore<R>(handle: SemaphoreHandle, f: impl FnOnce(&GuestSemaphore) -> R) -> Option<R> {
    let found = semaphores().lock().ok()?.get(&handle).map(Arc::clone);
    found.map(|s| f(&s))
}

/// Takes one, waiting as long as `until` allows. `None` when the handle names nothing.
pub fn semaphore_wait(handle: SemaphoreHandle, need: u32, until: Blocking) -> Option<bool> {
    with_semaphore(handle, |s| s.take(need, until))
}

/// How many the semaphore has free. `None` when the handle names nothing.
pub fn semaphore_value(handle: SemaphoreHandle) -> Option<u32> {
    with_semaphore(handle, GuestSemaphore::value).flatten()
}

/// Returns `n` to the semaphore.
pub fn semaphore_signal(handle: SemaphoreHandle, n: u32) -> Option<bool> {
    with_semaphore(handle, |s| s.signal(n))
}

/// Forgets a semaphore. `false` when the handle names nothing.
pub fn semaphore_destroy(handle: SemaphoreHandle) -> bool {
    semaphores()
        .lock()
        .is_ok_and(|mut t| t.remove(&handle).is_some())
}

/// What a semaphore was called, if it names one.
pub fn semaphore_name_of(handle: SemaphoreHandle) -> Option<String> {
    with_semaphore(handle, |s| s.name.clone())
}

/// Every lock the guest has made.
///
/// Locks are behind an `Arc` so one can be taken out of the table and used with the table
/// released (see [`with`]).
fn table() -> &'static Mutex<BTreeMap<MutexHandle, Arc<GuestMutex>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<MutexHandle, Arc<GuestMutex>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Hands out lock handles: the address of a fresh zeroed block, never freed.
fn next_handle() -> MutexHandle {
    orbistoun_mem::blocks::block(crate::thread::CONTROL_BLOCK_WORDS)
}

/// Creates a lock and returns the handle the guest should hold.
pub fn create(recursion: Recursion, name: &str) -> MutexHandle {
    let handle = next_handle();
    if let Ok(mut table) = table().lock() {
        table.insert(
            handle,
            Arc::new(GuestMutex::new(recursion, name.to_owned())),
        );
    }
    handle
}

/// Runs `f` against a lock, or answers `None` if the handle names nothing.
///
/// The table lock is released before `f` runs. Holding it across a blocking acquisition would
/// deadlock the process: the waiter sleeps holding the table, so the owner cannot reach the table
/// to release the lock.
fn with<R>(handle: MutexHandle, f: impl FnOnce(&GuestMutex) -> R) -> Option<R> {
    let found = table().lock().ok()?.get(&handle).map(Arc::clone);
    found.map(|m| f(&m))
}

/// Takes a lock, waiting as long as `until` allows.
///
/// `None` when the handle names nothing; otherwise the outcome, which distinguishes a busy lock
/// from a self-deadlock on an error-checking one (see [`Acquisition::Busy`]).
pub fn acquire(handle: MutexHandle, by: ThreadHandle, until: Blocking) -> Option<Acquisition> {
    with(handle, |m| m.acquire(by, until))
}

/// Releases a lock.
pub fn unlock(handle: MutexHandle, by: ThreadHandle) -> Option<bool> {
    with(handle, |m| m.unlock(by))
}

/// Forgets a lock. A handle held by the guest afterwards is a lookup miss.
pub fn destroy(handle: MutexHandle) -> bool {
    table()
        .lock()
        .is_ok_and(|mut t| t.remove(&handle).is_some())
}

/// The name the guest gave a lock, for a trace.
pub fn name_of(handle: MutexHandle) -> Option<String> {
    with(handle, |m| m.name.clone())
}

/// What a guest holds for a condition variable.
pub type CondHandle = u64;

/// A guest condition variable, and the host one it waits on.
struct GuestCond {
    /// What it was called, for a trace to name.
    name: String,
    /// The host primitive.
    signal: Condvar,
    /// How many wakes are owed.
    ///
    /// Counted rather than relying on the host notify alone, so a signal sent before anybody waits
    /// is kept.
    pending: Mutex<u64>,
}

fn conds() -> &'static Mutex<BTreeMap<CondHandle, Arc<GuestCond>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<CondHandle, Arc<GuestCond>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// A block this crate owns, whose address the guest holds as a handle, as for a mutex.
fn new_handle() -> u64 {
    orbistoun_mem::blocks::block(4)
}

/// Creates a condition variable, answering the handle the guest holds.
pub fn create_cond(name: &str) -> CondHandle {
    let handle = new_handle();
    if let Ok(mut table) = conds().lock() {
        table.insert(
            handle,
            Arc::new(GuestCond {
                name: name.to_owned(),
                signal: Condvar::new(),
                pending: Mutex::new(0),
            }),
        );
    }
    handle
}

/// Runs `f` against a condition variable, with the table released first.
fn with_cond<R>(handle: CondHandle, f: impl FnOnce(&GuestCond) -> R) -> Option<R> {
    let found = conds().lock().ok()?.get(&handle).map(Arc::clone);
    found.map(|c| f(&c))
}

/// Waits until signalled, or until `timeout` passes when one is given.
///
/// The guest mutex is not touched here: POSIX releases and reacquires it atomically around the
/// wait, and the caller does both around this call, so a signal arriving in that gap is lost
/// where on the platform it would not be.
///
/// The owed-wake count is the condition and a wake only prompts a re-read, because a condition
/// variable may wake a waiter spuriously. The deadline is re-read each turn so repeated wakes
/// cannot extend the total wait.
pub fn cond_wait(handle: CondHandle, timeout: Option<Duration>) -> Option<bool> {
    with_cond(handle, |c| {
        let Ok(mut pending) = c.pending.lock() else {
            return false;
        };
        // A span so large the clock cannot represent the moment becomes no deadline at all.
        let deadline = timeout.and_then(|span| Instant::now().checked_add(span));
        loop {
            if *pending > 0 {
                *pending -= 1;
                return true;
            }
            let Some(until) = deadline else {
                let Ok(next) = c.signal.wait(pending) else {
                    return false;
                };
                pending = next;
                continue;
            };
            let remaining = until.saturating_duration_since(Instant::now());
            let Ok((next, outcome)) = c.signal.wait_timeout(pending, remaining) else {
                return false;
            };
            pending = next;
            if outcome.timed_out() && *pending == 0 {
                return false;
            }
        }
    })
}

/// Wakes one waiter, or records that one wake is owed.
pub fn cond_signal(handle: CondHandle) -> Option<bool> {
    with_cond(handle, |c| {
        let Ok(mut pending) = c.pending.lock() else {
            return false;
        };
        *pending += 1;
        c.signal.notify_one();
        true
    })
}

/// Wakes every waiter.
pub fn cond_broadcast(handle: CondHandle) -> Option<bool> {
    with_cond(handle, |c| {
        let Ok(mut pending) = c.pending.lock() else {
            return false;
        };
        *pending += 1;
        c.signal.notify_all();
        true
    })
}

/// Forgets a condition variable.
pub fn cond_destroy(handle: CondHandle) -> bool {
    conds()
        .lock()
        .is_ok_and(|mut t| t.remove(&handle).is_some())
}

/// What a condition variable was called.
pub fn cond_name_of(handle: CondHandle) -> Option<String> {
    with_cond(handle, |c| c.name.clone())
}

/// What a guest holds for a read/write lock.
pub type RwlockHandle = u64;

/// Who holds a read/write lock right now.
#[derive(Default)]
struct RwState {
    /// How many readers hold it. Zero when a writer does, or when it is free.
    readers: u32,
    /// Whether a writer holds it.
    writer: bool,
}

/// A guest read/write lock.
struct GuestRwlock {
    name: String,
    state: Mutex<RwState>,
    /// Signalled whenever the lock is released, so a blocked acquirer re-checks.
    released: Condvar,
}

fn rwlocks() -> &'static Mutex<BTreeMap<RwlockHandle, Arc<GuestRwlock>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<RwlockHandle, Arc<GuestRwlock>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Creates a read/write lock.
pub fn create_rwlock(name: &str) -> RwlockHandle {
    let handle = new_handle();
    if let Ok(mut table) = rwlocks().lock() {
        table.insert(
            handle,
            Arc::new(GuestRwlock {
                name: name.to_owned(),
                state: Mutex::new(RwState::default()),
                released: Condvar::new(),
            }),
        );
    }
    handle
}

/// Runs `f` against a read/write lock, with the table released first.
fn with_rwlock<R>(handle: RwlockHandle, f: impl FnOnce(&GuestRwlock) -> R) -> Option<R> {
    let found = rwlocks().lock().ok()?.get(&handle).map(Arc::clone);
    found.map(|l| f(&l))
}

/// Takes the lock for reading, blocking while a writer holds it.
///
/// Readers do not wait for other readers.
pub fn rwlock_read(handle: RwlockHandle, until: Blocking) -> Option<bool> {
    with_rwlock(handle, |l| {
        let Ok(state) = l.state.lock() else {
            return false;
        };
        let Some(mut state) = wait_while(&l.released, state, until, |s| s.writer) else {
            return false;
        };
        state.readers += 1;
        true
    })
}

/// Takes the lock for writing, blocking while anybody holds it.
pub fn rwlock_write(handle: RwlockHandle, until: Blocking) -> Option<bool> {
    with_rwlock(handle, |l| {
        let Ok(state) = l.state.lock() else {
            return false;
        };
        let taken = wait_while(&l.released, state, until, |s| s.writer || s.readers > 0);
        let Some(mut state) = taken else {
            return false;
        };
        state.writer = true;
        true
    })
}

/// Releases whichever way it was held.
///
/// The guest unlock is one call for both, so a writer release is inferred from the writer flag
/// and anything else decrements the readers. A release by somebody holding nothing is reported
/// as the guest bug it is.
pub fn rwlock_unlock(handle: RwlockHandle) -> Option<bool> {
    with_rwlock(handle, |l| {
        let Ok(mut state) = l.state.lock() else {
            return false;
        };
        if state.writer {
            state.writer = false;
        } else if state.readers > 0 {
            state.readers -= 1;
        } else {
            return false;
        }
        l.released.notify_all();
        true
    })
}

/// Forgets a read/write lock.
pub fn rwlock_destroy(handle: RwlockHandle) -> bool {
    rwlocks()
        .lock()
        .is_ok_and(|mut t| t.remove(&handle).is_some())
}

/// What a read/write lock was called.
pub fn rwlock_name_of(handle: RwlockHandle) -> Option<String> {
    with_rwlock(handle, |l| l.name.clone())
}

/// What a guest holds for a barrier.
pub type BarrierHandle = u64;

/// A guest barrier.
struct GuestBarrier {
    name: String,
    /// How many must arrive before any may leave.
    needed: u32,
    /// Arrived so far, and which round this is.
    ///
    /// The round number stops a fast thread re-entering the barrier from being counted into a round
    /// a slow one has not yet left.
    state: Mutex<(u32, u64)>,
    released: Condvar,
}

fn barriers() -> &'static Mutex<BTreeMap<BarrierHandle, Arc<GuestBarrier>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<BarrierHandle, Arc<GuestBarrier>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Creates a barrier that releases once `needed` threads have arrived.
///
/// A count of zero would never release, so it is treated as one.
pub fn create_barrier(needed: u32, name: &str) -> BarrierHandle {
    let handle = new_handle();
    if let Ok(mut table) = barriers().lock() {
        table.insert(
            handle,
            Arc::new(GuestBarrier {
                name: name.to_owned(),
                needed: needed.max(1),
                state: Mutex::new((0, 0)),
                released: Condvar::new(),
            }),
        );
    }
    handle
}

/// Waits at the barrier. Answers whether this call was the one that released it.
pub fn barrier_wait(handle: BarrierHandle) -> Option<bool> {
    let found = barriers().lock().ok()?.get(&handle).map(Arc::clone)?;
    let Ok(mut state) = found.state.lock() else {
        return Some(false);
    };
    let round = state.1;
    state.0 += 1;
    if state.0 >= found.needed {
        state.0 = 0;
        state.1 = round.wrapping_add(1);
        found.released.notify_all();
        return Some(true);
    }
    while state.1 == round {
        let Ok(next) = found.released.wait(state) else {
            return Some(false);
        };
        state = next;
    }
    Some(false)
}

/// Forgets a barrier.
pub fn barrier_destroy(handle: BarrierHandle) -> bool {
    barriers()
        .lock()
        .is_ok_and(|mut t| t.remove(&handle).is_some())
}

/// What a barrier was called.
pub fn barrier_name_of(handle: BarrierHandle) -> Option<String> {
    let found = barriers().lock().ok()?.get(&handle).map(Arc::clone)?;
    Some(found.name.clone())
}

/// What a guest holds for an event queue.
pub type EqueueHandle = u64;

/// A guest event queue: a named place events are delivered to and waited on.
struct GuestEqueue {
    name: String,
    /// Events registered against this queue as `(identifier, udata)`, in arrival order.
    ///
    /// `udata` is the caller's own opaque word, which `kevent` echoes back on every delivery
    /// (D560).
    registered: Mutex<Vec<(u64, u64)>>,
    /// Events posted and not yet collected, oldest first.
    pending: Mutex<VecDeque<PendingEvent>>,
    /// Waits begun against this queue.
    ///
    /// A queue waited on and never posted to is a stuck thread, and without this count it looks like
    /// an unused queue.
    waited: AtomicU64,
    /// Events actually handed to a caller.
    delivered: AtomicU64,
    /// Signalled when something is posted, so a wait blocks until an event arrives.
    arrived: Condvar,
}

/// One event, in the fields a guest reads back out of a delivered one.
///
/// `sceKernelWaitEqueue` is `kevent(2)` on a FreeBSD-derived kernel, so the layout is FreeBSD's
/// `struct kevent` (D560): `ident` u64 at 0x00, `filter` i16 at 0x08, `flags` u16 at 0x0a,
/// `fflags` u32 at 0x0c, `data` i64 at 0x10, `udata` u64 at 0x18. FreeBSD 12 appended
/// `uint64_t ext[4]`, making it 0x40 bytes; the fields here are common to both, and
/// [`EVENT_BYTES`] uses the 0x20 form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingEvent {
    /// What the event is about - for a flip, the video-out port handle.
    pub ident: u64,
    /// Which kind of event this is.
    pub filter: i16,
    /// Action flags.
    pub flags: u16,
    /// Filter-specific flags.
    pub fflags: u32,
    /// Filter-specific data.
    pub data: i64,
    /// The opaque word the caller supplied when it registered.
    pub udata: u64,
}

/// How many bytes one delivered event occupies: the pre-FreeBSD-12 `struct kevent`.
///
/// The 0x40-byte form is the alternative if a guest reads past 0x20.
pub const EVENT_BYTES: usize = 0x20;

impl PendingEvent {
    /// The event as the guest reads it, in the layout [`PendingEvent`] documents.
    #[must_use]
    pub fn to_bytes(self) -> [u8; EVENT_BYTES] {
        let mut out = [0u8; EVENT_BYTES];
        out[0x00..0x08].copy_from_slice(&self.ident.to_le_bytes());
        out[0x08..0x0a].copy_from_slice(&self.filter.to_le_bytes());
        out[0x0a..0x0c].copy_from_slice(&self.flags.to_le_bytes());
        out[0x0c..0x10].copy_from_slice(&self.fflags.to_le_bytes());
        out[0x10..0x18].copy_from_slice(&self.data.to_le_bytes());
        out[0x18..0x20].copy_from_slice(&self.udata.to_le_bytes());
        out
    }
}

fn equeues() -> &'static Mutex<BTreeMap<EqueueHandle, Arc<GuestEqueue>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<EqueueHandle, Arc<GuestEqueue>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Creates an event queue under a caller-supplied name.
pub fn create_equeue(name: &str) -> EqueueHandle {
    let handle = new_handle();
    if let Ok(mut table) = equeues().lock() {
        table.insert(
            handle,
            Arc::new(GuestEqueue {
                name: name.to_owned(),
                registered: Mutex::new(Vec::new()),
                pending: Mutex::new(VecDeque::new()),
                waited: AtomicU64::new(0),
                delivered: AtomicU64::new(0),
                arrived: Condvar::new(),
            }),
        );
    }
    handle
}

/// Registers `identifier` against a queue, answering whether the queue exists.
///
/// A registration against a queue nobody created is refused, rather than reporting success for
/// a queue that will never deliver anything (D524).
pub fn register_event(handle: EqueueHandle, identifier: u64) -> bool {
    register_event_with_udata(handle, identifier, 0)
}

/// The same, carrying the caller's opaque word so a delivery can echo it.
pub fn register_event_with_udata(handle: EqueueHandle, identifier: u64, udata: u64) -> bool {
    let found = equeues()
        .lock()
        .ok()
        .and_then(|t| t.get(&handle).map(Arc::clone));
    let Some(queue) = found else {
        return false;
    };
    if let Ok(mut ids) = queue.registered.lock() {
        ids.push((identifier, udata));
    }
    true
}

/// Posts an event to every queue that registered `ident`, answering how many took it.
///
/// Routing by registered identifier lets the completing subsystem (a flip, for example) post
/// without knowing which queue a title waits on. Zero means nothing had registered, so the
/// completion had no reader.
pub fn post_event(ident: u64, event: PendingEvent) -> usize {
    let Ok(table) = equeues().lock() else {
        return 0;
    };
    let mut posted = 0;
    for queue in table.values() {
        let Ok(ids) = queue.registered.lock() else {
            continue;
        };
        let Some(&(_, udata)) = ids.iter().find(|(id, _)| *id == ident) else {
            continue;
        };
        drop(ids);
        if let Ok(mut pending) = queue.pending.lock() {
            // The caller's own word, echoed back the way `kevent` echoes it.
            pending.push_back(PendingEvent { udata, ..event });
            posted += 1;
            drop(pending);
            // Woken after the push and outside the lock. A waiter that wakes to an empty queue sleeps
            // again; one that is never woken waits forever.
            queue.arrived.notify_all();
        }
    }
    posted
}

/// Posts an event to every queue, ignoring what is registered.
///
/// Only for the `ORBISTOUN_FLIP_TO_ALL` diagnostic, which asks what a guest blocked on a queue
/// nothing feeds would do if that wait completed. Returns how many queues it reached, like
/// [`post_event`].
pub fn post_event_everywhere(event: PendingEvent) -> usize {
    let Ok(table) = equeues().lock() else {
        return 0;
    };
    let mut posted = 0;
    for queue in table.values() {
        // The registration's `udata` where there is one; a queue with none gets the event as given.
        let udata = queue
            .registered
            .lock()
            .ok()
            .and_then(|ids| ids.first().map(|(_, u)| *u))
            .unwrap_or(event.udata);
        if let Ok(mut pending) = queue.pending.lock() {
            pending.push_back(PendingEvent { udata, ..event });
            posted += 1;
            drop(pending);
            queue.arrived.notify_all();
        }
    }
    posted
}

/// Collects up to `wanted` events from a queue, oldest first, without blocking.
///
/// Empty when the queue has none, which is not an error: `kevent` with nothing ready reports
/// zero delivered.
pub fn take_events(handle: EqueueHandle, wanted: usize) -> Vec<PendingEvent> {
    wait_events(handle, wanted, Blocking::Never).unwrap_or_default()
}

/// Collects up to `wanted` events, waiting for the first one if the caller asked to.
///
/// `sceKernelWaitEqueue` is `kevent(2)`, which blocks until at least one event is ready or the
/// timeout elapses, so a guest looping until an event arrives blocks rather than spins.
///
/// `None` means nothing arrived within the caller's patience, distinct from an empty vector so
/// the call can refuse rather than claim success. A handle naming no queue is also `None`; the
/// caller checks that first.
pub fn wait_events(
    handle: EqueueHandle,
    wanted: usize,
    until: Blocking,
) -> Option<Vec<PendingEvent>> {
    let queue = equeues().lock().ok()?.get(&handle).map(Arc::clone)?;
    // Counted before the wait, so a wait that never returns is still counted.
    queue.waited.fetch_add(1, Ordering::Relaxed);
    let pending = queue.pending.lock().ok()?;
    let mut pending = wait_while(&queue.arrived, pending, until, VecDeque::is_empty)?;
    let take = wanted.min(pending.len());
    queue.delivered.fetch_add(take as u64, Ordering::Relaxed);
    Some(pending.drain(..take).collect())
}

/// Whether a queue exists at all, so a wait on a handle nobody was given can be refused.
#[must_use]
pub fn equeue_exists(handle: EqueueHandle) -> bool {
    equeues().lock().is_ok_and(|t| t.contains_key(&handle))
}

/// What a queue is called and what has been registered against it, for a run report.
#[must_use]
pub fn equeue_summary() -> Vec<EqueueTraffic> {
    let Ok(table) = equeues().lock() else {
        return Vec::new();
    };
    table
        .iter()
        .map(|(handle, q)| EqueueTraffic {
            handle: *handle,
            name: q.name.clone(),
            registered: q.registered.lock().map_or(0, |ids| ids.len()),
            waited: q.waited.load(Ordering::Relaxed),
            delivered: q.delivered.load(Ordering::Relaxed),
        })
        .collect()
}

/// What one event queue was used for, for a run report.
///
/// Registrations with no waits is a queue set up and unused; waits with no deliveries is a stuck
/// thread; both is a working queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EqueueTraffic {
    /// The handle the guest holds, which is what every argument dump shows, so the two records join.
    pub handle: EqueueHandle,
    /// What the guest called it.
    pub name: String,
    /// Events registered against it.
    pub registered: usize,
    /// Waits begun on it.
    pub waited: u64,
    /// Events handed to a caller out of it.
    pub delivered: u64,
}

/// What a guest holds for an Ult runtime or waiting-queue pool.
pub type UltHandle = u64;

/// A libSceUlt object a guest constructed: a runtime, or a pool of waiting-queue resources.
///
/// Issued from a table so a handle the guest passes back can be checked (D524); the guest's own
/// name is kept for the run report. The counts are recorded, not acted on, since no fibre
/// scheduler exists; storing them keeps the work-area size and the create consistent.
struct GuestUltObject {
    name: String,
    /// The first count the caller sized it by: threads.
    threads: u64,
    /// The second - sync objects for a pool, worker threads for a runtime.
    second: u64,
}

fn ult_objects() -> &'static Mutex<BTreeMap<UltHandle, Arc<GuestUltObject>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<UltHandle, Arc<GuestUltObject>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Constructs a named Ult object and returns the handle a guest stores in its first word.
pub fn create_ult_object(name: &str, threads: u64, second: u64) -> UltHandle {
    let handle = new_handle();
    if let Ok(mut table) = ult_objects().lock() {
        table.insert(
            handle,
            Arc::new(GuestUltObject {
                name: name.to_owned(),
                threads,
                second,
            }),
        );
    }
    handle
}

/// Whether a handle is one this crate issued for an Ult object.
#[must_use]
pub fn ult_object_exists(handle: UltHandle) -> bool {
    ult_objects().lock().is_ok_and(|t| t.contains_key(&handle))
}

/// What each Ult object is called and how it was sized, for a run report.
#[must_use]
pub fn ult_object_summary() -> Vec<(String, u64, u64)> {
    let Ok(table) = ult_objects().lock() else {
        return Vec::new();
    };
    table
        .values()
        .map(|o| (o.name.clone(), o.threads, o.second))
        .collect()
}

/// What a guest holds for an event flag.
pub type EventFlagHandle = u64;

/// A guest event flag: a word of bits threads wait on.
struct GuestEventFlag {
    name: String,
    bits: Mutex<u64>,
    changed: Condvar,
}

fn event_flags() -> &'static Mutex<BTreeMap<EventFlagHandle, Arc<GuestEventFlag>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<EventFlagHandle, Arc<GuestEventFlag>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Creates an event flag with an initial pattern.
pub fn create_event_flag(initial: u64, name: &str) -> EventFlagHandle {
    let handle = new_handle();
    if let Ok(mut table) = event_flags().lock() {
        table.insert(
            handle,
            Arc::new(GuestEventFlag {
                name: name.to_owned(),
                bits: Mutex::new(initial),
                changed: Condvar::new(),
            }),
        );
    }
    handle
}

/// Runs `f` against an event flag, with the table released first.
fn with_event_flag<R>(handle: EventFlagHandle, f: impl FnOnce(&GuestEventFlag) -> R) -> Option<R> {
    let found = event_flags().lock().ok()?.get(&handle).map(Arc::clone);
    found.map(|e| f(&e))
}

/// Tests the pattern without waiting, answering the bits at the moment of the test.
///
/// Nested because a handle naming nothing (a bad handle) and a pattern not set (an ordinary miss)
/// are different answers.
pub fn event_flag_poll(handle: EventFlagHandle, wanted: u64, all: bool) -> Option<Option<u64>> {
    with_event_flag(handle, |e| {
        let bits = *e.bits.lock().ok()?;
        let matched = if all {
            wanted != 0 && bits & wanted == wanted
        } else {
            bits & wanted != 0
        };
        matched.then_some(bits)
    })
}

/// Whether the handle names an event flag at all.
///
/// Separate from polling because the hardware checks the handle first: `0x80020003` for a handle
/// it never issued even when the mode is also invalid, and `0x80020016` for a bad mode on a valid
/// handle.
pub fn event_flag_exists(handle: EventFlagHandle) -> bool {
    with_event_flag(handle, |_| ()).is_some()
}
/// Sets bits and wakes anybody waiting.
pub fn event_flag_set(handle: EventFlagHandle, pattern: u64) -> Option<bool> {
    with_event_flag(handle, |e| {
        let Ok(mut bits) = e.bits.lock() else {
            return false;
        };
        *bits |= pattern;
        e.changed.notify_all();
        true
    })
}

/// Blocks the calling thread until the pattern is set, or the timeout elapses.
///
/// The blocking sibling of [`event_flag_poll`]: `all` selects AND (every bit of the pattern) over
/// OR (any bit); `clear_all` and `clear_pat` say what to clear on a match. `timeout` is [`None`] for
/// an indefinite wait. Answers as the poll does, so a caller tells three cases apart - outer [`None`]
/// for a handle naming nothing, inner [`None`] for a timeout, inner [`Some`] for the pattern found
/// (before any clear).
///
/// The thread parks on the [`Condvar`] that [`event_flag_set`] notifies. The `bits` lock is
/// released across the wait, so a setter is never shut out.
pub fn event_flag_wait(
    handle: EventFlagHandle,
    wanted: u64,
    all: bool,
    clear_all: bool,
    clear_pat: bool,
    timeout: Option<Duration>,
) -> Option<Option<u64>> {
    // The Arc is cloned out with the table released, as in `with_event_flag`, so only this flag's
    // `bits` lock is involved in the wait.
    let found = event_flags().lock().ok()?.get(&handle).map(Arc::clone)?;
    let mut bits = found.bits.lock().ok()?;
    let matches = |value: u64| {
        if all {
            wanted != 0 && value & wanted == wanted
        } else {
            value & wanted != 0
        }
    };
    let deadline = timeout.map(|d| Instant::now() + d);
    loop {
        if matches(*bits) {
            let found_pattern = *bits;
            if clear_all {
                *bits = 0;
            } else if clear_pat {
                *bits &= !wanted;
            }
            return Some(Some(found_pattern));
        }
        match deadline {
            None => {
                bits = found.changed.wait(bits).ok()?;
            }
            Some(when) => {
                let now = Instant::now();
                if now >= when {
                    return Some(None);
                }
                let (next, timed_out) = found.changed.wait_timeout(bits, when - now).ok()?;
                bits = next;
                if timed_out.timed_out() && !matches(*bits) {
                    return Some(None);
                }
            }
        }
    }
}

/// Clears every bit outside `pattern`, which is what the interface clear does.
pub fn event_flag_clear(handle: EventFlagHandle, pattern: u64) -> Option<bool> {
    with_event_flag(handle, |e| {
        let Ok(mut bits) = e.bits.lock() else {
            return false;
        };
        *bits &= pattern;
        true
    })
}

/// Forgets an event flag.
pub fn event_flag_destroy(handle: EventFlagHandle) -> bool {
    event_flags()
        .lock()
        .is_ok_and(|mut t| t.remove(&handle).is_some())
}

/// What an event flag was called.
pub fn event_flag_name_of(handle: EventFlagHandle) -> Option<String> {
    with_event_flag(handle, |e| e.name.clone())
}

/// What a wait on an address came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressWait {
    /// The word already held something other than what the caller expected, so there was nothing
    /// to sleep for. Not a failure: the guest re-reads the word, since the wake may have come first.
    Mismatch,
    /// A wake on this address arrived while sleeping.
    Woken,
    /// Patience ran out with no wake, or the caller would not wait at all.
    TimedOut,
}

/// The threads asleep on one address, and the wakes handed to them.
#[derive(Debug, Default)]
struct AddressQueue {
    /// Threads currently asleep here.
    waiting: u32,
    /// Wakes granted and not yet collected by a sleeper.
    ///
    /// Never more than `waiting`: a wake with nobody asleep leaves nothing behind, as in the futex
    /// contract, where the word carries the state and a wake only ends a sleep (D573). A stray token
    /// would wake a later, unrelated sleeper.
    tokens: u32,
}

/// One address's queue: the count under a lock, and the condition its sleepers wait on.
struct AddressWaiters {
    state: Mutex<AddressQueue>,
    woken: Condvar,
}

/// Every address anything has waited on.
///
/// Entries are never removed: dropping one while a sleeper still holds its `Arc` would leave that
/// sleeper on a queue no wake can find. The set is bounded by the distinct addresses a guest
/// waits on.
fn address_waiters() -> &'static Mutex<BTreeMap<u64, Arc<AddressWaiters>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<u64, Arc<AddressWaiters>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// How a wait finds out about, and runs, a signal raised on the sleeping thread.
///
/// This module knows nothing about guest memory or guest threads, and running a guest signal
/// handler needs the handler table and a way to call guest code, both above here. So they are
/// installed as function pointers, like the thread-start hook. Without them a wait ignores
/// signals.
#[derive(Debug, Clone, Copy)]
pub struct SignalDelivery {
    /// Whether the calling thread has a signal waiting. Lock-free, because it is called inside a
    /// condition-variable predicate under the queue's own lock.
    pub pending: fn() -> bool,
    /// Runs whatever is waiting on the calling thread. Called with no lock held, because it runs
    /// guest code that may take any lock in here.
    pub deliver: fn(),
}

/// The installed delivery, if a run wired one up.
static DELIVERY: OnceLock<SignalDelivery> = OnceLock::new();

/// Installs signal delivery for waits. Called once, by whoever owns the run.
pub fn install_signal_delivery(delivery: SignalDelivery) {
    let _ = DELIVERY.set(delivery);
}

/// Whether the calling thread has a signal waiting, or `false` when nothing is installed.
fn signal_pending() -> bool {
    DELIVERY.get().is_some_and(|d| (d.pending)())
}

/// Runs a signal waiting on the calling thread, if delivery is installed.
fn deliver_signal() {
    if let Some(delivery) = DELIVERY.get() {
        (delivery.deliver)();
    }
}

/// Nudges every address queue, so a thread with a signal pending re-tests its predicate.
///
/// Every queue, because a raise knows its target thread but not which word it sleeps on.
/// Each other sleeper re-tests once and sleeps again, once per raise. The queue's lock is
/// taken before notifying, so the flag cannot be set between a sleeper's last test and its
/// sleep.
pub fn nudge_address_waiters() {
    let queues: Vec<Arc<AddressWaiters>> = match address_waiters().lock() {
        Ok(table) => table.values().cloned().collect(),
        Err(_) => return,
    };
    for queue in queues {
        if let Ok(state) = queue.state.lock() {
            drop(state);
            queue.woken.notify_all();
        }
    }
}

/// The queue for `address`, made if this is the first wait on it.
fn address_queue(address: u64) -> Option<Arc<AddressWaiters>> {
    let mut table = address_waiters().lock().ok()?;
    Some(Arc::clone(table.entry(address).or_insert_with(|| {
        Arc::new(AddressWaiters {
            state: Mutex::new(AddressQueue::default()),
            woken: Condvar::new(),
        })
    })))
}

/// Sleeps while the word at `address` holds `expected`, until a wake on the same address or
/// patience runs out.
///
/// `read` fetches the word under the queue's lock, so a waker that writes the word and then
/// wakes always finds a sleeper that read the old value; a read before the lock could miss the
/// wake. As a closure it also keeps guest memory in the crate root and lets this module be
/// tested with a word it owns (D573).
///
/// `None` only for a host lock that is poisoned or a word that could not be read.
pub fn wait_on_address(
    address: u64,
    expected: u64,
    read: impl FnOnce() -> Option<u64>,
    until: Blocking,
) -> Option<AddressWait> {
    let queue = address_queue(address)?;
    let mut state = queue.state.lock().ok()?;
    if read()? != expected {
        return Some(AddressWait::Mismatch);
    }
    state.waiting += 1;
    // A loop, because a signal is not a wake: a thread woken to run a handler still waits for its
    // word, so it runs the handler and sleeps again (D652).
    let state = loop {
        let Some(woken) = wait_while(&queue.woken, state, until, |q| {
            q.tokens == 0 && !signal_pending()
        }) else {
            break None;
        };
        // The signal first, even when a wake arrived with it; otherwise it stays pending until a later
        // wait, which may never come.
        if !signal_pending() {
            break Some(woken);
        }
        // The lock is dropped first: the handler is guest code and may wait on this very queue.
        drop(woken);
        deliver_signal();
        state = queue.state.lock().ok()?;
    };
    let Some(mut state) = state else {
        // Gave up and dropped the guard. Re-take the lock to leave the queue; a wake that landed in the
        // gap counted this thread, so its token is collected here rather than left for a stranger.
        let mut state = queue.state.lock().ok()?;
        state.waiting -= 1;
        if state.tokens > state.waiting {
            state.tokens -= 1;
            return Some(AddressWait::Woken);
        }
        return Some(AddressWait::TimedOut);
    };
    state.tokens -= 1;
    state.waiting -= 1;
    Some(AddressWait::Woken)
}

/// Wakes up to `count` threads asleep on `address`, and says how many that was.
///
/// Zero is the ordinary answer: a wake races the wait by design and the word carries the state.
/// `None` only for a poisoned host lock.
pub fn wake_on_address(address: u64, count: u32) -> Option<u32> {
    let queue = address_waiters().lock().ok()?.get(&address).cloned();
    let Some(queue) = queue else {
        return Some(0);
    };
    let mut state = queue.state.lock().ok()?;
    let granted = count.min(state.waiting.saturating_sub(state.tokens));
    if granted > 0 {
        state.tokens += granted;
        queue.woken.notify_all();
    }
    Some(granted)
}

/// How many threads are asleep on `address` right now.
///
/// A snapshot, for reports and tests; anything may wake or sleep before the caller acts on it.
pub fn waiting_on_address(address: u64) -> u32 {
    let Ok(table) = address_waiters().lock() else {
        return 0;
    };
    table
        .get(&address)
        .and_then(|q| q.state.lock().ok().map(|s| s.waiting))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{Acquisition, Blocking, Recursion, acquire, create, destroy, name_of, unlock};

    #[test]
    fn a_handle_is_memory_the_guest_can_read_through() {
        // A small integer handle faults when a guest reads a field through it, so the handle is a real
        // readable block.
        let m = create(Recursion::Forbidden, "dereferenced");
        assert_ne!(m, super::NO_MUTEX);
        assert_eq!(m % 8, 0, "and it must be aligned for a word read");

        // SAFETY: `m` is the address of a leaked, zeroed, eight-byte-aligned block this
        // module owns and never frees, so reading a word from it is always valid.
        let first_word = unsafe { std::ptr::read(m as usize as *const u64) };
        assert_eq!(
            first_word, 0,
            "unknown fields must read as zero, not garbage"
        );
    }

    #[test]
    fn a_lock_is_held_until_it_is_released() {
        let m = create(Recursion::Forbidden, "held");
        assert_eq!(acquire(m, 1, Blocking::Forever), Some(Acquisition::Locked));
        // A different thread must not get in while it is held.
        assert_eq!(
            acquire(m, 2, Blocking::Never),
            Some(Acquisition::Busy),
            "held locks are not available"
        );
        assert_eq!(unlock(m, 1), Some(true));
        assert_eq!(
            acquire(m, 2, Blocking::Never),
            Some(Acquisition::Locked),
            "and available once released"
        );
    }

    #[test]
    fn a_non_recursive_lock_refuses_its_owner_rather_than_deadlocking() {
        // Blocking here would deadlock the thread against itself.
        let m = create(Recursion::Forbidden, "once");
        assert_eq!(acquire(m, 1, Blocking::Forever), Some(Acquisition::Locked));
        assert_eq!(
            acquire(m, 1, Blocking::Forever),
            Some(Acquisition::Busy),
            "a second take must be refused"
        );
    }

    #[test]
    fn a_recursive_lock_counts_its_acquisitions() {
        // Releasing on the first unlock would let another thread in while the owner is still inside.
        let m = create(Recursion::Allowed, "nested");
        assert_eq!(acquire(m, 1, Blocking::Forever), Some(Acquisition::Locked));
        assert_eq!(acquire(m, 1, Blocking::Forever), Some(Acquisition::Locked));
        assert_eq!(unlock(m, 1), Some(true));
        assert_eq!(
            acquire(m, 2, Blocking::Never),
            Some(Acquisition::Busy),
            "still held at depth one"
        );
        assert_eq!(unlock(m, 1), Some(true));
        assert_eq!(acquire(m, 2, Blocking::Never), Some(Acquisition::Locked));
    }

    #[test]
    fn a_thread_cannot_release_a_lock_it_does_not_hold() {
        // Otherwise two guest threads could be inside one critical section.
        let m = create(Recursion::Forbidden, "owned");
        assert_eq!(acquire(m, 1, Blocking::Forever), Some(Acquisition::Locked));
        assert_eq!(unlock(m, 2), Some(false), "not this thread's to release");
        assert_eq!(
            acquire(m, 2, Blocking::Never),
            Some(Acquisition::Busy),
            "and it is still held"
        );
    }

    #[test]
    fn an_unknown_handle_is_a_miss_rather_than_a_success() {
        // Success on a lock nobody made would let every guest thread through.
        assert_eq!(acquire(0, 1, Blocking::Forever), None);
        assert_eq!(unlock(u64::MAX, 1), None);
    }

    #[test]
    fn a_destroyed_lock_stops_answering() {
        let m = create(Recursion::Forbidden, "gone");
        assert!(destroy(m));
        assert_eq!(
            acquire(m, 1, Blocking::Forever),
            None,
            "a stale handle is a miss, not a lock"
        );
        assert!(!destroy(m), "and destroying it twice reports the truth");
    }

    #[test]
    fn a_lock_remembers_the_name_the_guest_gave_it() {
        // Names make locks distinguishable in a trace.
        let m = create(Recursion::Forbidden, "render-queue");
        assert_eq!(name_of(m).as_deref(), Some("render-queue"));
    }

    #[test]
    fn a_lock_actually_excludes_a_real_thread() {
        // The tests above run on one thread, where a lock that did nothing would still pass; this one
        // blocks a second host thread on it.
        use std::sync::atomic::{AtomicBool, Ordering};
        static ENTERED: AtomicBool = AtomicBool::new(false);

        let m = create(Recursion::Forbidden, "contended");
        assert_eq!(acquire(m, 1, Blocking::Forever), Some(Acquisition::Locked));

        let waiter = std::thread::spawn(move || {
            acquire(m, 2, Blocking::Forever);
            ENTERED.store(true, Ordering::SeqCst);
            unlock(m, 2);
        });

        // Give the waiter every chance to get in wrongly before the release.
        std::thread::yield_now();
        assert!(
            !ENTERED.load(Ordering::SeqCst),
            "the second thread must still be waiting"
        );

        assert_eq!(unlock(m, 1), Some(true));
        waiter.join().expect("the waiter should be released");
        assert!(ENTERED.load(Ordering::SeqCst), "and then it gets in");
    }

    #[test]
    fn a_semaphore_hands_out_its_initial_count_and_then_refuses() {
        let h = super::create_semaphore(2, 4, "startup");
        assert_eq!(super::semaphore_wait(h, 1, Blocking::Never), Some(true));
        assert_eq!(super::semaphore_wait(h, 1, Blocking::Never), Some(true));
        assert_eq!(
            super::semaphore_wait(h, 1, Blocking::Never),
            Some(false),
            "empty, and an impatient take must say so rather than wait"
        );
        assert_eq!(super::semaphore_signal(h, 1), Some(true));
        assert_eq!(super::semaphore_wait(h, 1, Blocking::Never), Some(true));
    }

    #[test]
    fn signalling_past_the_ceiling_is_refused_rather_than_clamped() {
        // Clamping would let a guest that has lost count carry on as though it had not.
        let h = super::create_semaphore(0, 2, "bounded");
        assert_eq!(super::semaphore_signal(h, 2), Some(true));
        assert_eq!(super::semaphore_signal(h, 1), Some(false));
    }

    #[test]
    fn a_handle_that_names_nothing_answers_none_rather_than_a_default() {
        // `None` says the handle was never issued, a different fault from a refused operation.
        assert_eq!(super::semaphore_wait(0x7fff_beef, 1, Blocking::Never), None);
        assert!(!super::semaphore_destroy(0x7fff_beef));
    }

    /// A semaphore handle survives the four-byte write the guest's `int` receives.
    ///
    /// Mixing semaphore and mutex handles is a type error; this checks what types cannot, that a
    /// semaphore handle stays small where a host pointer would not.
    #[test]
    fn a_semaphore_handle_fits_the_int_the_guest_holds() {
        let s = super::create_semaphore(1, 1, "a semaphore");
        assert_ne!(s, super::NO_SEMAPHORE, "zero still means nothing here");
        assert!(
            s > 0 && s < 0x0001_0000,
            // `concat!` defeats implicit `{name}` capture, so the argument goes positional.
            concat!(
                "a handle of {} is a counter gone wrong - the guest holds this in an int, ",
                "and the whole reason for a counter is that a host address does not fit"
            ),
            s
        );
        // Round-trips through the four bytes the guest keeps.
        #[allow(clippy::cast_possible_truncation)]
        let narrowed = s as i32;
        assert_eq!(
            narrowed, s,
            "the handle must survive the width it is stored at"
        );
    }

    /// A delivered event puts each field where FreeBSD's `struct kevent` puts it.
    ///
    /// Asserted field by field at its offset, since a round trip through this project's own encoder
    /// would agree with itself whatever the offsets were. It pins what orbistoun writes against the
    /// reference, not that the hardware agrees.
    #[test]
    fn a_delivered_event_matches_the_published_kevent_layout() {
        let bytes = super::PendingEvent {
            ident: 0x1122_3344_5566_7788,
            filter: -3,
            flags: 0xbeef,
            fflags: 0xdead_0001,
            data: -2,
            udata: 0x0102_0304_0506_0708,
        }
        .to_bytes();

        assert_eq!(&bytes[0x00..0x08], &0x1122_3344_5566_7788u64.to_le_bytes());
        assert_eq!(&bytes[0x08..0x0a], &(-3i16).to_le_bytes(), "filter at 0x08");
        assert_eq!(
            &bytes[0x0a..0x0c],
            &0xbeefu16.to_le_bytes(),
            "flags at 0x0a"
        );
        assert_eq!(&bytes[0x0c..0x10], &0xdead_0001u32.to_le_bytes());
        assert_eq!(&bytes[0x10..0x18], &(-2i64).to_le_bytes(), "data at 0x10");
        assert_eq!(&bytes[0x18..0x20], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(bytes.len(), super::EVENT_BYTES);
    }

    /// A completion reaches only the queues that registered for it.
    ///
    /// A flip knows its port, not which queue a title waits on, so registration decides; a post to
    /// every queue would deliver completions to queues used for something else.
    #[test]
    fn a_posted_event_reaches_only_queues_that_registered_for_it() {
        let listening = super::create_equeue("listening");
        let indifferent = super::create_equeue("indifferent");
        let ident = 0x5150;
        assert!(super::register_event(listening, ident));

        let event = super::PendingEvent {
            ident,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 7,
            udata: 0,
        };
        assert_eq!(super::post_event(ident, event), 1, "one queue wanted it");

        assert_eq!(super::take_events(listening, 4).len(), 1);
        assert!(
            super::take_events(indifferent, 4).is_empty(),
            "a queue that registered nothing was handed an event anyway"
        );
    }

    /// A post nobody registered for reaches no queue, and says so.
    #[test]
    fn a_completion_with_no_reader_posts_to_nothing() {
        let ident = 0x5151;
        let event = super::PendingEvent {
            ident,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: 0,
        };
        assert_eq!(
            super::post_event(ident, event),
            0,
            "an unregistered completion must report that it had no reader"
        );
    }

    /// The caller's own word is echoed from its registration, not from the poster.
    ///
    /// The poster does not know `udata`, so taking it from the post would always return zero.
    #[test]
    fn the_registrations_udata_is_what_comes_back() {
        let queue = super::create_equeue("echo");
        let ident = 0x5152;
        assert!(super::register_event_with_udata(queue, ident, 0xabcd_ef01));

        super::post_event(
            ident,
            super::PendingEvent {
                ident,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                // Deliberately wrong: the poster does not know this and must not decide it.
                udata: 0xdead_dead,
            },
        );
        let delivered = super::take_events(queue, 1);
        assert_eq!(delivered.len(), 1);
        assert_eq!(
            delivered[0].udata, 0xabcd_ef01,
            "the poster's word was handed back instead of the registration's"
        );
    }

    /// Events come back oldest first, and no more than were asked for.
    #[test]
    fn events_are_delivered_oldest_first_and_bounded_by_the_request() {
        let queue = super::create_equeue("ordered");
        let ident = 0x5153;
        assert!(super::register_event(queue, ident));
        for data in 1..=3i64 {
            super::post_event(
                ident,
                super::PendingEvent {
                    ident,
                    filter: 0,
                    flags: 0,
                    fflags: 0,
                    data,
                    udata: 0,
                },
            );
        }

        let first = super::take_events(queue, 1);
        assert_eq!(first.len(), 1, "one was asked for");
        assert_eq!(first[0].data, 1, "and it must be the oldest");
        let rest = super::take_events(queue, 8);
        assert_eq!(
            rest.iter().map(|e| e.data).collect::<Vec<_>>(),
            vec![2, 3],
            "the remainder, still in order, and asking for eight does not invent five"
        );
        assert!(
            super::take_events(queue, 8).is_empty(),
            "a drained queue keeps delivering events"
        );
    }

    /// A wait on a queue nobody created is not an empty queue.
    ///
    /// Nothing ready is an ordinary poll; an unknown handle is a value orbistoun never issued.
    #[test]
    fn an_unknown_queue_is_distinguishable_from_an_empty_one() {
        let real = super::create_equeue("real");
        assert!(super::equeue_exists(real));
        assert!(super::take_events(real, 1).is_empty(), "real but empty");
        assert!(
            !super::equeue_exists(0xdead_beef_0bad_0bad),
            "a handle nothing issued must not look like a queue"
        );
    }
}

#[cfg(test)]
mod address_tests {
    //! The futex contract. The word is a test-owned atomic rather than guest memory, which the
    //! read closure allows.

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{AddressWait, Blocking, wait_on_address, waiting_on_address, wake_on_address};

    /// Addresses are keys, so each test takes its own and no wake crosses between them.
    fn fresh_address() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(0x7400_0000_0000);
        NEXT.fetch_add(0x100, Ordering::Relaxed)
    }

    /// Waits until `n` threads are asleep on `address`, or fails the test.
    fn settle(address: u64, n: u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while waiting_on_address(address) != n {
            assert!(
                Instant::now() < deadline,
                "sleepers never arrived on {address:#x}"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// A sleeper on `address` that expects the word to hold zero.
    fn sleeper(address: u64, word: &Arc<AtomicU64>) -> thread::JoinHandle<Option<AddressWait>> {
        let word = Arc::clone(word);
        thread::spawn(move || {
            wait_on_address(
                address,
                0,
                || Some(word.load(Ordering::SeqCst)),
                Blocking::Forever,
            )
        })
    }

    #[test]
    fn a_word_that_already_changed_is_not_waited_on() {
        // The wake came first and the word says so; sleeping here would be a lost wakeup.
        let address = fresh_address();
        let got = wait_on_address(address, 0, || Some(1), Blocking::Forever);
        assert_eq!(got, Some(AddressWait::Mismatch));
        assert_eq!(
            waiting_on_address(address),
            0,
            "nothing was left on the queue"
        );
    }

    #[test]
    fn a_sleeper_is_woken_by_a_wake_on_its_own_address() {
        let address = fresh_address();
        let word = Arc::new(AtomicU64::new(0));
        let asleep = sleeper(address, &word);
        settle(address, 1);
        word.store(1, Ordering::SeqCst);
        assert_eq!(
            wake_on_address(address, 1),
            Some(1),
            "one sleeper, one woken"
        );
        assert_eq!(asleep.join().ok().flatten(), Some(AddressWait::Woken));
        assert_eq!(waiting_on_address(address), 0);
    }

    #[test]
    fn a_wake_with_nobody_asleep_is_not_remembered() {
        // The word carries the state and the wake only ends a sleep. A token kept from this wake would
        // end the timed wait below early, which the `TimedOut` assertion catches.
        let address = fresh_address();
        assert_eq!(wake_on_address(address, 1), Some(0));
        let until = Blocking::Until(Instant::now() + Duration::from_millis(40));
        let got = wait_on_address(address, 0, || Some(0), until);
        assert_eq!(got, Some(AddressWait::TimedOut));
        assert_eq!(
            waiting_on_address(address),
            0,
            "a timed-out sleeper leaves the queue"
        );
    }

    #[test]
    fn a_wake_of_one_leaves_the_other_asleep() {
        // A count of one wakes one thread; waking more is a spurious wakeup the guest did not ask for.
        let address = fresh_address();
        let word = Arc::new(AtomicU64::new(0));
        let first = sleeper(address, &word);
        let second = sleeper(address, &word);
        settle(address, 2);
        assert_eq!(wake_on_address(address, 1), Some(1));
        // Exactly one wakes; the queue settles at one sleeper rather than zero.
        settle(address, 1);
        assert_eq!(wake_on_address(address, 1), Some(1));
        assert_eq!(first.join().ok().flatten(), Some(AddressWait::Woken));
        assert_eq!(second.join().ok().flatten(), Some(AddressWait::Woken));
    }

    #[test]
    fn a_wake_on_another_address_does_not_count() {
        let address = fresh_address();
        let other = fresh_address();
        let word = Arc::new(AtomicU64::new(0));
        let asleep = sleeper(address, &word);
        settle(address, 1);
        assert_eq!(wake_on_address(other, 1), Some(0), "nobody sleeps there");
        assert_eq!(
            waiting_on_address(address),
            1,
            "and the real sleeper is untouched"
        );
        assert_eq!(wake_on_address(address, 1), Some(1));
        assert_eq!(asleep.join().ok().flatten(), Some(AddressWait::Woken));
    }

    #[test]
    fn a_wake_asking_for_more_than_are_asleep_reports_what_it_woke() {
        // The answer is how many woke, not how many were asked for.
        let address = fresh_address();
        let word = Arc::new(AtomicU64::new(0));
        let asleep = sleeper(address, &word);
        settle(address, 1);
        assert_eq!(wake_on_address(address, 8), Some(1));
        assert_eq!(asleep.join().ok().flatten(), Some(AddressWait::Woken));
        // And nothing was left over for the next sleeper to trip on.
        let until = Blocking::Until(Instant::now() + Duration::from_millis(40));
        assert_eq!(
            wait_on_address(address, 0, || Some(0), until),
            Some(AddressWait::TimedOut)
        );
    }

    #[test]
    fn a_caller_that_will_not_wait_is_refused_rather_than_slept() {
        let address = fresh_address();
        let got = wait_on_address(address, 0, || Some(0), Blocking::Never);
        assert_eq!(got, Some(AddressWait::TimedOut));
        assert_eq!(waiting_on_address(address), 0);
    }

    #[test]
    fn an_unreadable_word_is_a_miss_rather_than_a_sleep() {
        // A word that cannot be read is not slept on.
        let address = fresh_address();
        assert_eq!(
            wait_on_address(address, 0, || None, Blocking::Forever),
            None
        );
        assert_eq!(waiting_on_address(address), 0);
    }
}
