//! Guest synchronisation primitives.
//!
//! # Why a host `Mutex` cannot be used directly
//!
//! Rust's mutex hands out a guard whose lifetime *is* the critical section, which is
//! exactly the property that makes it safe and exactly the property that makes it
//! unusable here. The guest locks in one call and unlocks in a different one, with
//! arbitrary guest code - possibly other calls into this crate - in between. There is no
//! host frame to hold a guard in.
//!
//! So the lock is built from a mutex over a small state and a condition variable: the
//! host mutex is held only while the state is inspected, never across the guest's
//! critical section. That also makes it honest about ownership, which matters more than
//! it sounds - a recursive lock taken twice by one thread must not deadlock, and a
//! non-recursive one taken twice must not silently succeed.
//!
//! # What the guest holds
//!
//! The address of a zeroed block this crate owns, written into the location the guest
//! passed to the init call - the same shape as a thread handle, and for the same reason
//! (see `thread::ThreadHandle`). A small integer would be cheaper and would fault the
//! moment a guest read a field through it.
//!
//! The block's contents are never written, because the real layout is not known from any
//! lawful source. Reading a field gives zero rather than something invented.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::thread::{NO_THREAD, ThreadHandle};

/// How the guest refers to a lock.
pub type MutexHandle = u64;

/// Handle meaning "no lock", and what a lookup miss looks like.
pub const NO_MUTEX: MutexHandle = 0;

/// What a guest holds for a semaphore.
///
/// **A different shape from a mutex handle, and that is the whole point of the type.** A
/// mutex is a `void *` and a semaphore is an `int` written through an out-pointer: four
/// bytes, not eight (obSCEne, D210). Sharing `MutexHandle` for both meant this crate wrote
/// a host pointer through a pointer to a four-byte field, putting the top half of it in
/// whatever the guest kept next door.
pub type SemaphoreHandle = i32;

/// Sentinel for "no semaphore here".
pub const NO_SEMAPHORE: SemaphoreHandle = 0;

/// Whether a lock may be taken twice by the thread already holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Recursion {
    /// A second acquisition by the owner is an error.
    ///
    /// The default because it is POSIX's, and because the alternative turns a real
    /// double-lock bug in the guest into silence.
    #[default]
    Forbidden,
    /// The owner may acquire it repeatedly, and must release it as many times.
    Allowed,
    /// A second acquisition by the owner is reported as a deadlock, not blocked and not
    /// allowed. The platform's error-checking mutex, measured to answer a distinct code from a
    /// plain busy on a self-`trylock` (015-sync/mutex-recursion).
    Errorcheck,
}

/// The three answers a `try_lock` can give, which a two-state `bool` could not hold: the owner
/// re-taking a `Forbidden` lock is *busy*, and re-taking an `Errorcheck` one is a *deadlock*, and
/// the platform gives those two different codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryLock {
    /// Taken - free, or a recursive re-take by the owner.
    Locked,
    /// Held, by another thread or by the owner of a non-recursive lock.
    Busy,
    /// The owner re-taking an error-checking lock, which is a deadlock it is told about.
    Deadlock,
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

    /// Takes the lock, blocking until it is free.
    ///
    /// Returns `false` for the one case that must not block: a non-recursive lock being
    /// taken again by the thread that already owns it. Waiting there would deadlock
    /// against ourselves and look like a hang in the guest.
    fn lock(&self, by: ThreadHandle) -> bool {
        let Ok(mut held) = self.state.lock() else {
            return false;
        };
        if held.depth > 0 && held.owner == by {
            if self.recursion != Recursion::Allowed {
                // Forbidden and Errorcheck both refuse a self-relock rather than nest or block.
                return false;
            }
            held.depth += 1;
            return true;
        }
        while held.depth > 0 {
            let Ok(next) = self.released.wait(held) else {
                return false;
            };
            held = next;
        }
        held.owner = by;
        held.depth = 1;
        true
    }

    /// Takes the lock only if it is free right now, distinguishing busy from a self-deadlock.
    fn try_lock(&self, by: ThreadHandle) -> TryLock {
        let Ok(mut held) = self.state.lock() else {
            return TryLock::Busy;
        };
        if held.depth == 0 {
            held.owner = by;
            held.depth = 1;
            return TryLock::Locked;
        }
        if held.owner == by {
            return match self.recursion {
                Recursion::Allowed => {
                    held.depth += 1;
                    TryLock::Locked
                }
                Recursion::Errorcheck => TryLock::Deadlock,
                Recursion::Forbidden => TryLock::Busy,
            };
        }
        // Held by another thread.
        TryLock::Busy
    }

    /// Releases the lock.
    ///
    /// Returns `false` when the caller does not hold it. **Refusing rather than
    /// releasing** is the point: unlocking somebody else's lock would let two guest
    /// threads into the same critical section, and the corruption that follows would be
    /// attributed to whatever they were protecting rather than to here (principle 3).
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
/// # Why this is here now and was not before
///
/// Phase 5 says build the synchronisation primitives when a guest asks for one, and one
/// finally did: `sceKernelCreateSema` is the single import whose error return aborted two
/// titles during static initialisation, and its name came out of a *third* title's own
/// bytes (D193).
///
/// Same shape as [`GuestMutex`] and for the same reason: the guest creates it in one call
/// and uses it in another, so the object outlives any host guard. A `Condvar` carries the
/// waiters, and the count is plain because every operation on it happens under the lock.
#[derive(Debug)]
struct GuestSemaphore {
    state: Mutex<u32>,
    available: Condvar,
    /// The most the count may reach. Nothing enforces it yet; recorded so a signal past
    /// the ceiling can be refused rather than silently accepted once the guest does one.
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

    /// Takes one, blocking until there is one to take.
    fn wait(&self) -> bool {
        let Ok(mut count) = self.state.lock() else {
            return false;
        };
        while *count == 0 {
            let Ok(next) = self.available.wait(count) else {
                return false;
            };
            count = next;
        }
        *count -= 1;
        true
    }

    /// Takes one only if one is free.
    fn try_wait(&self) -> bool {
        let Ok(mut count) = self.state.lock() else {
            return false;
        };
        if *count == 0 {
            return false;
        }
        *count -= 1;
        true
    }

    /// Returns `n`, refusing to exceed the ceiling.
    ///
    /// **Refused rather than clamped.** Silently capping would let a guest that has lost
    /// count carry on as though it had not, and the imbalance would surface as a hang
    /// somewhere with no connection to here (principle 3).
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

/// Every semaphore the guest has made.
/// The next semaphore handle.
///
/// A counter, not a leaked pointer. Mutex handles are host addresses, which is fine for a
/// `void *` and impossible for an `int` - a 48-bit address truncated to four bytes collides
/// with every other semaphore that shares its low half, and does so silently.
///
/// Starts at one, so zero keeps meaning "nothing here" for a field a guest zeroed.
fn next_semaphore_handle() -> SemaphoreHandle {
    static NEXT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
/// The same rule the mutex table follows, and for the same reason: holding the table
/// across a blocking wait deadlocks every thread that would have signalled it.
fn with_semaphore<R>(handle: SemaphoreHandle, f: impl FnOnce(&GuestSemaphore) -> R) -> Option<R> {
    let found = semaphores().lock().ok()?.get(&handle).map(Arc::clone);
    found.map(|s| f(&s))
}

/// Takes one, blocking until available. `None` when the handle names nothing.
pub fn semaphore_wait(handle: SemaphoreHandle) -> Option<bool> {
    with_semaphore(handle, GuestSemaphore::wait)
}

/// Takes one only if it is free.
pub fn semaphore_try_wait(handle: SemaphoreHandle) -> Option<bool> {
    with_semaphore(handle, GuestSemaphore::try_wait)
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
/// The locks are behind an `Arc` so one can be taken *out* of the table and used with
/// the table released - see [`with`], where that is the difference between working and
/// deadlocking the whole process.
fn table() -> &'static Mutex<BTreeMap<MutexHandle, Arc<GuestMutex>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<MutexHandle, Arc<GuestMutex>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Hands out lock handles: the address of a fresh zeroed block, never freed.
fn next_handle() -> MutexHandle {
    let block: Box<[u64; crate::thread::CONTROL_BLOCK_WORDS]> =
        Box::new([0; crate::thread::CONTROL_BLOCK_WORDS]);
    std::ptr::from_mut(Box::leak(block)) as usize as u64
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
/// **The table lock is released before `f` runs**, and the first version of this got it
/// wrong. Holding the table across a blocking acquisition deadlocks the entire process:
/// the waiter sleeps on the lock's condition variable still holding the table, so the
/// owner cannot reach the table to release it, so the waiter never wakes. It passed
/// every single-threaded test.
fn with<R>(handle: MutexHandle, f: impl FnOnce(&GuestMutex) -> R) -> Option<R> {
    let found = table().lock().ok()?.get(&handle).map(Arc::clone);
    found.map(|m| f(&m))
}

/// Takes a lock, blocking until it is available.
///
/// `None` when the handle names nothing.
pub fn lock(handle: MutexHandle, by: ThreadHandle) -> Option<bool> {
    with(handle, |m| m.lock(by))
}

/// Takes a lock only if it is free. `None` when the handle names nothing; otherwise the outcome,
/// which distinguishes a busy lock from a self-deadlock on an error-checking one.
pub fn try_lock(handle: MutexHandle, by: ThreadHandle) -> Option<TryLock> {
    with(handle, |m| m.try_lock(by))
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

// --- condition variables -------------------------------------------------------------

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
    /// **Counted rather than relying on the host notify alone.** A guest may signal before
    /// anybody waits, and a count makes what happens next explicit rather than leaving it
    /// to host scheduling.
    pending: Mutex<u64>,
}

fn conds() -> &'static Mutex<BTreeMap<CondHandle, Arc<GuestCond>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<CondHandle, Arc<GuestCond>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// A block this crate owns, whose address the guest holds as a handle.
///
/// The same shape as a mutex handle and for the same reason: a small integer would be
/// cheaper and would fault the moment a guest read a field through it.
fn new_handle() -> u64 {
    let block: Box<[u64; 4]> = Box::new([0; 4]);
    std::ptr::from_mut(Box::leak(block)) as usize as u64
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
/// **The guest mutex is not touched here.** POSIX requires the wait to release it
/// atomically and reacquire it on return; these are independent objects in this crate, so
/// the caller does that around this call. A signal arriving in the gap is lost, where on
/// the platform it would not be - recorded rather than hidden.
pub fn cond_wait(handle: CondHandle, timeout: Option<std::time::Duration>) -> Option<bool> {
    with_cond(handle, |c| {
        let Ok(mut pending) = c.pending.lock() else {
            return false;
        };
        if *pending > 0 {
            *pending -= 1;
            return true;
        }
        if let Some(limit) = timeout {
            let Ok((mut guard, outcome)) = c.signal.wait_timeout(pending, limit) else {
                return false;
            };
            if outcome.timed_out() {
                return false;
            }
            if *guard > 0 {
                *guard -= 1;
            }
            return true;
        }
        let Ok(mut guard) = c.signal.wait(pending) else {
            return false;
        };
        if *guard > 0 {
            *guard -= 1;
        }
        true
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

// --- read/write locks ----------------------------------------------------------------

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
/// **Readers do not wait for other readers**, which is the whole point of the type: a
/// shared lock that queued readers behind each other would be a mutex wearing another name.
pub fn rwlock_read(handle: RwlockHandle, blocking: bool) -> Option<bool> {
    with_rwlock(handle, |l| {
        let Ok(mut state) = l.state.lock() else {
            return false;
        };
        while state.writer {
            if !blocking {
                return false;
            }
            let Ok(next) = l.released.wait(state) else {
                return false;
            };
            state = next;
        }
        state.readers += 1;
        true
    })
}

/// Takes the lock for writing, blocking while anybody holds it.
pub fn rwlock_write(handle: RwlockHandle, blocking: bool) -> Option<bool> {
    with_rwlock(handle, |l| {
        let Ok(mut state) = l.state.lock() else {
            return false;
        };
        while state.writer || state.readers > 0 {
            if !blocking {
                return false;
            }
            let Ok(next) = l.released.wait(state) else {
                return false;
            };
            state = next;
        }
        state.writer = true;
        true
    })
}

/// Releases whichever way it was held.
///
/// **Not told which**, because the guest unlock is one call for both - so a writer release
/// is inferred from the writer flag and anything else decrements the readers. A release by
/// somebody holding nothing is reported rather than ignored: it is a real bug in the guest
/// and silence would let it corrupt whatever the lock was protecting.
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

// --- barriers ------------------------------------------------------------------------

/// What a guest holds for a barrier.
pub type BarrierHandle = u64;

/// A guest barrier.
struct GuestBarrier {
    name: String,
    /// How many must arrive before any may leave.
    needed: u32,
    /// Arrived so far, and which round this is.
    ///
    /// The round number stops a fast thread re-entering the barrier and being counted
    /// twice while a slow one has not yet woken from the previous release.
    state: Mutex<(u32, u64)>,
    released: Condvar,
}

fn barriers() -> &'static Mutex<BTreeMap<BarrierHandle, Arc<GuestBarrier>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<BarrierHandle, Arc<GuestBarrier>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Creates a barrier that releases once `needed` threads have arrived.
///
/// A count of zero would never release, which is a hang rather than an error - so it is
/// treated as one, which releases immediately and is visible.
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

// --- event flags ---------------------------------------------------------------------

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
/// **A handle naming nothing and a pattern that is simply not set are different answers**,
/// which is why this nests: the first is a bad handle and the second is an ordinary miss,
/// and a guest branches differently on each.
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

#[cfg(test)]
mod tests {
    use super::{Recursion, TryLock, create, destroy, lock, name_of, try_lock, unlock};

    #[test]
    fn a_handle_is_memory_the_guest_can_read_through() {
        // A small integer handle faults the moment a guest reads a field through it -
        // which is exactly how an unimplemented `scePthreadSelf` was caught, returning
        // an error code that a title then dereferenced.
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
        assert_eq!(lock(m, 1), Some(true));
        // A different thread must not get in while it is held.
        assert_eq!(
            try_lock(m, 2),
            Some(TryLock::Busy),
            "held locks are not available"
        );
        assert_eq!(unlock(m, 1), Some(true));
        assert_eq!(
            try_lock(m, 2),
            Some(TryLock::Locked),
            "and available once released"
        );
    }

    #[test]
    fn a_non_recursive_lock_refuses_its_owner_rather_than_deadlocking() {
        // Blocking here would deadlock the thread against itself and read as a hang in
        // the guest, with nothing naming the cause.
        let m = create(Recursion::Forbidden, "once");
        assert_eq!(lock(m, 1), Some(true));
        assert_eq!(lock(m, 1), Some(false), "a second take must be refused");
    }

    #[test]
    fn a_recursive_lock_counts_its_acquisitions() {
        // Releasing on the first unlock would let another thread in while the owner
        // still believes it is inside the critical section.
        let m = create(Recursion::Allowed, "nested");
        assert_eq!(lock(m, 1), Some(true));
        assert_eq!(lock(m, 1), Some(true));
        assert_eq!(unlock(m, 1), Some(true));
        assert_eq!(
            try_lock(m, 2),
            Some(TryLock::Busy),
            "still held at depth one"
        );
        assert_eq!(unlock(m, 1), Some(true));
        assert_eq!(try_lock(m, 2), Some(TryLock::Locked));
    }

    #[test]
    fn a_thread_cannot_release_a_lock_it_does_not_hold() {
        // The failure this prevents is two guest threads inside one critical section,
        // where the corruption gets blamed on whatever they were protecting.
        let m = create(Recursion::Forbidden, "owned");
        assert_eq!(lock(m, 1), Some(true));
        assert_eq!(unlock(m, 2), Some(false), "not this thread's to release");
        assert_eq!(try_lock(m, 2), Some(TryLock::Busy), "and it is still held");
    }

    #[test]
    fn an_unknown_handle_is_a_miss_rather_than_a_success() {
        // A stub that reported success on a lock nobody made would let every guest
        // thread through every critical section it names.
        assert_eq!(lock(0, 1), None);
        assert_eq!(unlock(u64::MAX, 1), None);
    }

    #[test]
    fn a_destroyed_lock_stops_answering() {
        let m = create(Recursion::Forbidden, "gone");
        assert!(destroy(m));
        assert_eq!(lock(m, 1), None, "a stale handle is a miss, not a lock");
        assert!(!destroy(m), "and destroying it twice reports the truth");
    }

    #[test]
    fn a_lock_remembers_the_name_the_guest_gave_it() {
        // Traces of unnamed locks are near-useless: every one looks the same.
        let m = create(Recursion::Forbidden, "render-queue");
        assert_eq!(name_of(m).as_deref(), Some("render-queue"));
    }

    #[test]
    fn a_lock_actually_excludes_a_real_thread() {
        // Every test above runs on one thread, where a lock that did nothing at all
        // would still pass. This one blocks a second host thread on it.
        use std::sync::atomic::{AtomicBool, Ordering};
        static ENTERED: AtomicBool = AtomicBool::new(false);

        let m = create(Recursion::Forbidden, "contended");
        assert_eq!(lock(m, 1), Some(true));

        let waiter = std::thread::spawn(move || {
            lock(m, 2);
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
        assert_eq!(super::semaphore_try_wait(h), Some(true));
        assert_eq!(super::semaphore_try_wait(h), Some(true));
        assert_eq!(
            super::semaphore_try_wait(h),
            Some(false),
            "empty, and the non-blocking form must say so rather than wait"
        );
        assert_eq!(super::semaphore_signal(h, 1), Some(true));
        assert_eq!(super::semaphore_try_wait(h), Some(true));
    }

    #[test]
    fn signalling_past_the_ceiling_is_refused_rather_than_clamped() {
        // Clamping would let a guest that has lost count carry on as though it had not,
        // and the imbalance would surface as a hang with no connection to here.
        let h = super::create_semaphore(0, 2, "bounded");
        assert_eq!(super::semaphore_signal(h, 2), Some(true));
        assert_eq!(super::semaphore_signal(h, 1), Some(false));
    }

    #[test]
    fn a_handle_that_names_nothing_answers_none_rather_than_a_default() {
        // `Some(false)` would read as "the operation failed"; `None` says the handle was
        // never one of ours, which is a different bug in a different place.
        assert_eq!(super::semaphore_try_wait(0x7fff_beef), None);
        assert!(!super::semaphore_destroy(0x7fff_beef));
    }

    /// The two handle spaces are different **types**, not merely different tables.
    ///
    /// This test used to pass a mutex handle to a semaphore call and assert it found
    /// nothing. It no longer compiles, which is a better answer: a semaphore handle is an
    /// `int` and a mutex handle is a `void *` (obSCEne, D210), so mixing them is now a build
    /// error rather than a lookup that happens to miss.
    ///
    /// What is left to check is the part types cannot: that a semaphore handle stays small
    /// enough to survive the four-byte write the guest's `int` receives. A host pointer does
    /// not, which is precisely what this crate was writing before.
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
        // Round-trips through the four bytes the guest actually keeps.
        #[allow(clippy::cast_possible_truncation)]
        let narrowed = s as i32;
        assert_eq!(
            narrowed, s,
            "the handle must survive the width it is stored at"
        );
    }
}
