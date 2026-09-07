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

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

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

/// The three answers an acquisition can give, which a two-state `bool` could not hold: the owner
/// re-taking a `Forbidden` lock is *busy*, and re-taking an `Errorcheck` one is a *deadlock*, and
/// the platform gives those two different codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acquisition {
    /// Taken - free, or a recursive re-take by the owner.
    Locked,
    /// Not taken: held by another thread, taken by the owner of a non-recursive lock, or -
    /// under [`Blocking::Until`] - still held when the deadline passed.
    ///
    /// **Which of those it was follows from the patience the caller asked for**, so it is not
    /// a fourth variant. A caller that passed [`Blocking::Never`] and got this was refused
    /// because the lock was busy; one that passed a deadline ran out of time. Only the caller
    /// knows which error name the guest is owed, and it is the caller that has to say it.
    Busy,
    /// The owner re-taking an error-checking lock, which is a deadlock it is told about.
    Deadlock,
}

// --- how long a call is willing to wait --------------------------------------------------

/// How long an acquisition may wait for what it wants.
///
/// **One parameter where there were three spellings.** Taking a lock, taking a semaphore and
/// taking a read-write lock are the same operation at different levels of patience, and each
/// had said so differently: the mutex by having a second entry point, the semaphore by having
/// a third, the read-write lock by a bare `bool`. A timed acquisition would have made that a
/// fourth spelling of one idea, so it became this instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocking {
    /// Take it if it is free this instant; refuse rather than wait.
    Never,
    /// Wait for as long as it takes.
    Forever,
    /// Wait until this moment, then give up.
    ///
    /// **An instant, not a span**, because a span is ambiguous about when it started - see
    /// `wait_while` for the bug that distinction prevents.
    Until(Instant),
}

/// Waits on `signal` until `blocked` stops holding of the guarded state, or patience runs out.
///
/// Answers `Some` holding the guard, with the predicate false; `None` when the wait gave up,
/// which is a refusal under [`Blocking::Never`], an expired deadline under
/// [`Blocking::Until`], and a poisoned host mutex under any of them.
///
/// # Why the deadline is re-read every turn
///
/// **Handing the whole remaining span to each `wait_timeout` would restart the clock on every
/// spurious wake.** A condition variable may wake a waiter with nothing to show for it - and
/// this module's do, because every release notifies all of them - so a lock under contention
/// would be waited on far past the deadline by a call that was asked to give up. Computing
/// what is left from a fixed instant is what makes the total bounded rather than each turn.
///
/// A deadline already past is a wait of no time rather than an error: the predicate above has
/// already had its look, so a lock that is free is still taken by a call that arrived late.
/// That is what POSIX asks for - the timeout of a `pthread_mutex_timedlock` is not consulted
/// when the mutex can be locked at once.
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
                // Timed out *and still blocked*: a wake that arrives with the deadline is
                // honoured if it brought what was wanted, which costs nothing and spares a
                // caller a spurious failure at the boundary.
                if outcome.timed_out() && blocked(&guard) {
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
    /// **The self-relock check comes before any waiting**, and must: a non-recursive lock
    /// being taken again by the thread that already owns it can never become free by
    /// waiting, so waiting there would deadlock against ourselves and look like a hang in
    /// the guest. It is answered the same way however patient the caller is.
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

    /// Takes one, waiting as long as `until` allows.
    fn take(&self, until: Blocking) -> bool {
        let Ok(count) = self.state.lock() else {
            return false;
        };
        let Some(mut count) = wait_while(&self.available, count, until, |c| *c == 0) else {
            return false;
        };
        *count -= 1;
        true
    }

    /// How many are free right now.
    ///
    /// **A snapshot, and true only of the instant it was taken** - anything may take one
    /// before the caller acts on the answer. That is the standard's own position on
    /// `sem_getvalue`, which says the value may already be stale when it is returned, so
    /// answering it is not a weaker contract than the platform's.
    fn value(&self) -> Option<u32> {
        self.state.lock().ok().map(|count| *count)
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

/// Takes one, waiting as long as `until` allows. `None` when the handle names nothing.
pub fn semaphore_wait(handle: SemaphoreHandle, until: Blocking) -> Option<bool> {
    with_semaphore(handle, |s| s.take(until))
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

/// Takes a lock, waiting as long as `until` allows.
///
/// `None` when the handle names nothing; otherwise the outcome, which distinguishes a busy
/// lock from a self-deadlock on an error-checking one. See [`Acquisition::Busy`] for why a
/// deadline that expired and a lock that was simply held share one answer.
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
///
/// # Why this loops rather than trusting a single wake
///
/// **A condition variable may wake a waiter with nothing to show for it**, and the first
/// version of this returned "signalled" for any wake at all. Under a loaded parallel test run
/// that produced two failures - an untimed waiter reporting it had been signalled before
/// anybody signalled, and a timed wait on a condition variable nothing ever touched reporting
/// success. Both passed when run alone, which is what a spurious wakeup looks like.
///
/// So the count is the condition and the wake is only a prompt to re-read it - the same
/// discipline `wait_while` applies to every other primitive here, and for the same reason.
/// The deadline is re-read each turn so a run of spurious wakes cannot extend the total wait.
pub fn cond_wait(handle: CondHandle, timeout: Option<Duration>) -> Option<bool> {
    with_cond(handle, |c| {
        let Ok(mut pending) = c.pending.lock() else {
            return false;
        };
        //
        // A span so large the clock cannot represent the moment becomes no deadline at all,
        // which is what a caller asking for it meant - and is nearer to the request than
        // answering immediately would be.
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

// --- event queues --------------------------------------------------------------------

/// What a guest holds for an event queue.
pub type EqueueHandle = u64;

/// A guest event queue: a named place events are delivered to and waited on.
///
/// # It has a queue now, and the note that said otherwise was stale
///
/// This said PPSA02664 *"never waits - `sceKernelWaitEqueue` is called zero times since the flip
/// count stopped lying (D516)"*, and declined the storage on those grounds. **That stopped being
/// true.** The title creates **four** queues, not two, and calls `sceKernelWaitEqueue` 839 times
/// in an honest run - 12,924 once it is past the shader wall. It was the largest unimplemented
/// call in the run by two orders of magnitude, and it was waiting for a flip completion nothing
/// ever posted (D560).
///
/// So the reader arrived, and the storage follows it rather than preceding it - which is what
/// the original note was protecting and is why it was right to write the number down.
struct GuestEqueue {
    name: String,
    /// Events registered against this queue as `(identifier, udata)`, in arrival order.
    ///
    /// `udata` is carried because `kevent` **echoes it back** on every delivery - it is the
    /// caller's own opaque word, and the one field of a delivered event whose value is not a
    /// guess here (D560).
    registered: Mutex<Vec<(u64, u64)>>,
    /// Events posted and not yet collected, oldest first.
    pending: Mutex<VecDeque<PendingEvent>>,
}

/// One event, in the fields a guest reads back out of a delivered one.
///
/// # Where the layout comes from
///
/// `sceKernelWaitEqueue` is `kevent(2)`, and the target kernel is FreeBSD-derived, so
/// `struct kevent` is the citable reference - the strongest oracle this project has
/// (principle 1). The fields and their order are FreeBSD's:
///
/// ```text
/// 0x00  u64  ident     what the event is about
/// 0x08  i16  filter    which kind of event
/// 0x0a  u16  flags     action flags
/// 0x0c  u32  fflags    filter-specific flags
/// 0x10  i64  data      filter-specific data
/// 0x18  u64  udata     the caller's own opaque word
/// ```
///
/// **The size is the open question, and it is named rather than assumed away.** FreeBSD 12 added
/// `uint64_t ext[4]`, taking the structure from 0x20 to 0x40 bytes. Everything above is common to
/// both, so a guest reading only these fields cannot tell them apart - and the one observed call
/// passes a buffer this project dumped 32 zeroed bytes from, which is consistent with 0x20 and
/// does not establish it. [`EVENT_BYTES`] is the value in play; if a guest ever reads past 0x20
/// the rival is the first thing to try (D560).
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

/// How many bytes one delivered event occupies.
///
/// The pre-FreeBSD-12 `struct kevent`. See [`PendingEvent`] for why the larger variant is
/// recorded as a rival rather than ruled out.
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
            }),
        );
    }
    handle
}

/// Registers `identifier` against a queue, answering whether the queue exists.
///
/// **The handle is checked**, which is the whole reason the table exists: a registration against
/// a queue nobody created is a guest holding a handle orbistoun never gave it, and accepting it
/// would report success for a queue that will never deliver anything.
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
/// # Why registration decides, rather than the caller naming a queue
///
/// A guest registers interest and then waits; the thing that *completes* - a flip, here - knows
/// what happened but not who asked. Routing by the registered identifier is what lets the video
/// layer post a completion without knowing which of four queues a title chose to wait on, which
/// is exactly the coupling a subsystem should not have (D560).
///
/// **Zero returned is a real answer.** It means nothing had registered for this, so the
/// completion had no reader - which is a fact worth having rather than a silent no-op.
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
            // The caller's own word, echoed back the way `kevent` echoes it - the one field of
            // a delivered event this project is not guessing at.
            pending.push_back(PendingEvent { udata, ..event });
            posted += 1;
        }
    }
    posted
}

/// Collects up to `wanted` events from a queue, oldest first.
///
/// Empty when the queue has none, and **that is not an error**: `kevent` with nothing ready
/// reports zero delivered, and a caller that asked for one and got none is told so rather than
/// handed a fabricated event. What this deliberately does *not* do is block - see
/// `sceKernelWaitEqueue`, where the reasoning belongs (D560).
pub fn take_events(handle: EqueueHandle, wanted: usize) -> Vec<PendingEvent> {
    let found = equeues()
        .lock()
        .ok()
        .and_then(|t| t.get(&handle).map(Arc::clone));
    let Some(queue) = found else {
        return Vec::new();
    };
    let Ok(mut pending) = queue.pending.lock() else {
        return Vec::new();
    };
    let take = wanted.min(pending.len());
    pending.drain(..take).collect()
}

/// Whether a queue exists at all, so a wait on a handle nobody was given can be refused.
#[must_use]
pub fn equeue_exists(handle: EqueueHandle) -> bool {
    equeues().lock().is_ok_and(|t| t.contains_key(&handle))
}

/// What a queue is called and what has been registered against it, for a run report.
#[must_use]
pub fn equeue_summary() -> Vec<(String, usize)> {
    let Ok(table) = equeues().lock() else {
        return Vec::new();
    };
    table
        .values()
        .map(|q| {
            let count = q.registered.lock().map_or(0, |ids| ids.len());
            (q.name.clone(), count)
        })
        .collect()
}

// --- libSceUlt runtimes and resource pools --------------------------------------------

/// What a guest holds for an Ult runtime or waiting-queue pool.
pub type UltHandle = u64;

/// A libSceUlt object a guest constructed: a runtime, or a pool of waiting-queue resources.
///
/// # Why there is a table rather than a bare `Ok`
///
/// The same reason D524 gave the event queues one: a handle that means nothing lets a guest pass
/// back something orbistoun never issued and be told it is fine. The name is the guest's own -
/// PPSA28061 builds a `"sample runtime"` and a `"waiting queue"` - and a run report reading those
/// back is how a person sees what a title set up (D564).
///
/// **The counts are recorded and not acted on.** How many fibres a runtime can hold is a property
/// of a scheduler that is not built; storing what was asked for is what lets the work-area size
/// and the create agree with each other.
struct GuestUltObject {
    name: String,
    /// The first count the caller sized it by - threads, in both observed calls.
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

/// Blocks the calling thread until the pattern is set, or the timeout elapses.
///
/// The blocking sibling of [`event_flag_poll`]: `all` selects AND (every bit of the pattern) over
/// OR (any bit); `clear_all` and `clear_pat` say what to clear on a match. `timeout` is [`None`] for
/// an indefinite wait. Answers as the poll does, so a caller tells three cases apart - outer [`None`]
/// for a handle naming nothing, inner [`None`] for a timeout, inner [`Some`] for the pattern found
/// (before any clear).
///
/// **This is where a guest thread actually blocks**, on the same [`Condvar`] [`event_flag_set`]
/// wakes, so a thread waiting on an event another thread sets is parked rather than spinning - which
/// is what an unimplemented wait had a guest doing, calling it hundreds of thousands of times
/// (PPSA04263). The `bits` lock is released across the wait by the condvar and re-taken on wake, so a
/// setter is never shut out.
pub fn event_flag_wait(
    handle: EventFlagHandle,
    wanted: u64,
    all: bool,
    clear_all: bool,
    clear_pat: bool,
    timeout: Option<Duration>,
) -> Option<Option<u64>> {
    // The Arc is cloned out with the table released first, exactly as `with_event_flag` does, so the
    // table is not held across the wait - only this flag's own `bits` lock is, and the condvar frees
    // that while parked.
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

// --- waiting on an address ------------------------------------------------------------

/// What a wait on an address came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressWait {
    /// The word already held something other than what the caller expected, so there was
    /// nothing to sleep for. Not a failure: the guest re-reads the word next, which is the
    /// whole contract - the wake may have come before the wait did.
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
    /// **Never more than `waiting`.** A wake with nobody asleep leaves nothing behind - that
    /// is the futex contract, in which the word carries the state and the wake only ends a
    /// sleep. A wait that arrives after the wake it needed re-reads the word, sees it changed,
    /// and does not sleep; one that reads the old value sleeps until the *next* wake. Keeping a
    /// stray token would wake a later, unrelated sleeper for no reason.
    tokens: u32,
}

/// One address's queue: the count under a lock, and the condition its sleepers wait on.
struct AddressWaiters {
    state: Mutex<AddressQueue>,
    woken: Condvar,
}

/// Every address anything has ever waited on.
///
/// Entries are never removed. Dropping one while a sleeper still holds its `Arc` would leave
/// that sleeper on a queue no wake can find - a lost wakeup with nothing in a trace to say so.
/// The set is bounded by the distinct addresses a guest waits on, which for the one title
/// measured is thirteen (D573).
fn address_waiters() -> &'static Mutex<BTreeMap<u64, Arc<AddressWaiters>>> {
    static TABLE: OnceLock<Mutex<BTreeMap<u64, Arc<AddressWaiters>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
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
/// `read` fetches the word. It is called **under the queue's lock**, which is the one thing
/// that makes this correct rather than merely usual: a waker writes the word and then wakes,
/// and a sleeper that read the old value has already joined the queue before the lock is
/// released into the sleep - so the wake finds it. Read before taking the lock, the wake could
/// land in the gap and the sleeper would wait for a second wake that never comes. That is why
/// the read is a closure rather than a value the caller looked up first.
///
/// The read is a closure for a second reason, the one principle 8 gives: guest memory stays in
/// the crate root, and this module can then be tested with a word it owns.
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
    let Some(mut state) = wait_while(&queue.woken, state, until, |q| q.tokens == 0) else {
        // Gave up, and the guard went with it. Re-take the lock to leave the queue - and
        // if a wake landed in that gap it counted this thread, so a token no remaining
        // sleeper can claim is this thread's and is collected rather than left to wake a
        // stranger later.
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
/// Zero is the ordinary answer, not an error: a wake races the wait by design, and the word
/// is what carries the state. `None` only for a poisoned host lock.
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
        // Blocking here would deadlock the thread against itself and read as a hang in
        // the guest, with nothing naming the cause.
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
        // Releasing on the first unlock would let another thread in while the owner
        // still believes it is inside the critical section.
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
        // The failure this prevents is two guest threads inside one critical section,
        // where the corruption gets blamed on whatever they were protecting.
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
        // A stub that reported success on a lock nobody made would let every guest
        // thread through every critical section it names.
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
        assert_eq!(super::semaphore_wait(h, Blocking::Never), Some(true));
        assert_eq!(super::semaphore_wait(h, Blocking::Never), Some(true));
        assert_eq!(
            super::semaphore_wait(h, Blocking::Never),
            Some(false),
            "empty, and an impatient take must say so rather than wait"
        );
        assert_eq!(super::semaphore_signal(h, 1), Some(true));
        assert_eq!(super::semaphore_wait(h, Blocking::Never), Some(true));
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
        assert_eq!(super::semaphore_wait(0x7fff_beef, Blocking::Never), None);
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

    /// **A delivered event puts each field where FreeBSD's `struct kevent` puts it.**
    ///
    /// The layout is the whole reason delivery was deferred (D524), so it is asserted field by
    /// field at its offset rather than by a round trip - a round trip through this project's own
    /// encoder would agree with itself whatever the offsets were.
    ///
    /// # What this cannot assert
    ///
    /// **That the console's structure is FreeBSD's.** This pins what orbistoun writes against
    /// the citable reference it was written from; the target is free to disagree, and D468 is
    /// this project watching exactly that happen to the ctype tables. It also cannot see the
    /// FreeBSD 12 variant, which appends `ext[4]` and leaves every offset here unchanged - that
    /// rival is recorded on [`PendingEvent`] and is invisible to any test of these six fields.
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

    /// **A completion reaches only the queues that registered for it.**
    ///
    /// The routing property the video layer depends on: a flip knows its port and not which of
    /// four queues a title chose, so registration is what decides. A post that reached every
    /// queue would deliver flip completions to the queue a title uses for something else, and
    /// nothing downstream could tell.
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

    /// **Nobody registered means nobody is told, and that is reported rather than swallowed.**
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

    /// **The caller's own word is echoed from its registration, not from the poster.**
    ///
    /// `kevent` hands `udata` back untouched, and it is the one field of a delivered event that
    /// is not a guess here. The poster does not know it - a flip knows the port, not what the
    /// waiter asked to have handed back - so taking it from the post would return zero for ever
    /// and look exactly like a title that passed zero.
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

    /// **Events come back oldest first, and no more than were asked for.**
    ///
    /// A guest asking for one at a time - which PPSA02664 does - must not have the queue
    /// drained, and must not be handed the newest event while an older one waits behind it.
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

    /// **A wait on a queue nobody created is not an empty queue.**
    ///
    /// The distinction the caller acts on: nothing ready is an ordinary poll, an unknown handle
    /// is a guest holding something orbistoun never issued.
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
    //! The futex contract, and each test states the failure it exists to catch. The word is a
    //! test-owned atomic rather than guest memory, which is what the read closure is for.

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
        // The wake came first and the word says so. Sleeping here is the lost-wakeup hang.
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
        // The futex contract: the word carries the state, the wake only ends a sleep. A token
        // kept from this wake would end the timed wait below early, and the assertion on
        // `TimedOut` is what would catch that - this test was watched failing with the guard
        // in `wake_on_address` removed.
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
        // Waking everybody on a count of one is a spurious wakeup the guest did not ask for,
        // and a guest whose thread pool hands out work one wake at a time would run two
        // workers on one job.
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
        // The answer is what happened, not what was asked for - a guest counting wakes
        // against workers would otherwise be told about workers that do not exist.
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
        // Sleeping on a word that could not be read would be waiting for a wake on an address
        // the guest may not even own, forever, with nothing naming the cause.
        let address = fresh_address();
        assert_eq!(
            wait_on_address(address, 0, || None, Blocking::Forever),
            None
        );
        assert_eq!(waiting_on_address(address), 0);
    }
}
