//! Guest threads.
//!
//! A guest thread is always a real host thread: guest code reads thread-local storage through
//! the segment base directly and blocks inside its own synchronisation primitives, which a
//! pooled implementation cannot present. The guest decides how many threads exist; the host's
//! core count only decides how many run at once, so there is no core minimum to enforce.
//!
//! The host's shape changes two things. [`CpuTopology`] tells the guest the target's core
//! counts by default, not the host's. An affinity request is mapped onto the host and the
//! original is kept (see [`AffinityPolicy`], D150).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// How the guest sees a thread.
///
/// The address of a real, zeroed block this crate owns, because the guest dereferences it
/// (D151).
pub type ThreadHandle = u64;

/// Handle given out when a thread could not be created.
///
/// Zero, because that is what a caller tests for.
pub const NO_THREAD: ThreadHandle = 0;

/// The shape of the machine a guest believes it is running on.
///
/// Configurable, because the target's shape and the host's answer different questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CpuTopology {
    /// Cores the guest is told exist.
    pub cores: u32,
    /// Cores the guest is told it may actually use.
    ///
    /// Lower than `cores` on the target, which keeps some for the system.
    pub usable: u32,
}

impl Default for CpuTopology {
    fn default() -> Self {
        // The target's shape, not the host's: a guest asking is asking about the machine it was
        // written for. These figures are an assumption, not a measurement.
        Self {
            cores: 8,
            usable: 7,
        }
    }
}

impl CpuTopology {
    /// What the machine orbistoun is running on has, for a developer measuring throughput.
    pub fn host() -> Self {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let cores = u32::try_from(cores).unwrap_or(1);
        Self {
            cores,
            usable: cores,
        }
    }
}

/// What to do with an affinity request.
///
/// Under every policy the requested mask is recorded on the thread, so a title that depends on
/// affinity can be found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AffinityPolicy {
    /// Record the request and let the host scheduler place the thread.
    ///
    /// The default: no title examined depends on placement, and the recorded requests are the
    /// evidence that would show one does.
    #[default]
    Observe,
    /// Fold the guest's mask onto the host's cores.
    ///
    /// Guest core `n` becomes host core `n % host`, which keeps threads pinned apart apart and
    /// gives up exact placement.
    Map,
    /// Apply the mask as given, and fail if the host cannot satisfy it.
    ///
    /// Never a default; it answers whether a title needs exactly what it asked for.
    Strict,
}

/// A core mask, as the guest expresses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Affinity(pub u64);

impl Affinity {
    /// Every core the guest named.
    pub fn cores(self) -> impl Iterator<Item = u32> {
        (0..64).filter(move |b| self.0 & (1 << b) != 0)
    }

    /// Whether the mask names nothing, which means "anywhere".
    pub const fn is_unset(self) -> bool {
        self.0 == 0
    }

    /// The mask this becomes on a host with `host_cores` cores, under `policy`.
    ///
    /// `None` means the request cannot be honoured and the caller should refuse, which happens
    /// only under [`AffinityPolicy::Strict`].
    pub fn mapped(self, policy: AffinityPolicy, host_cores: u32) -> Option<Self> {
        if self.is_unset() || host_cores == 0 {
            return Some(Self(0));
        }
        match policy {
            AffinityPolicy::Observe => Some(Self(0)),
            AffinityPolicy::Map => {
                // Fold rather than clamp: clamping collapses every out-of-range core onto the highest one and
                // puts threads the guest separated back together.
                let folded = self
                    .cores()
                    .map(|c| 1_u64 << (c % host_cores))
                    .fold(0, |a, b| a | b);
                Some(Self(folded))
            }
            AffinityPolicy::Strict => {
                let highest = self.cores().max().unwrap_or(0);
                (highest < host_cores).then_some(self)
            }
        }
    }
}

/// Everything about threading that is a choice rather than a fact.
///
/// Serialisable, so changing what the guest is told is a file edit and a relaunch, not a
/// rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    /// The machine the guest is told about.
    pub topology: CpuTopology,
    /// What happens to an affinity request.
    pub affinity: AffinityPolicy,
    /// Whether the guest's priority requests are applied to host threads.
    ///
    /// Off by default: raising a host thread's priority on a guest's request can starve the
    /// emulator's own threads.
    pub apply_priority: bool,
}

/// The settings in force.
fn settings() -> &'static Mutex<Settings> {
    static SETTINGS: OnceLock<Mutex<Settings>> = OnceLock::new();
    SETTINGS.get_or_init(|| Mutex::new(Settings::default()))
}

/// Replaces the threading settings.
///
/// Called once during setup. Threads already running keep the placement they were given.
pub fn configure(new: Settings) {
    if let Ok(mut current) = settings().lock() {
        *current = new;
    }
}

/// The settings in force.
pub fn configured() -> Settings {
    settings().lock().map(|s| *s).unwrap_or_default()
}

/// One guest thread, and what was asked for when it was made.
#[derive(Debug, Clone)]
pub struct ThreadRecord {
    /// The handle the guest holds.
    pub handle: ThreadHandle,
    /// The host thread this one is running on, or zero before it has run.
    ///
    /// Joins this table to the recorded calls, which carry a host thread, so a report can tell
    /// threads apart by name.
    pub host: u64,
    /// The name the guest gave it, if any.
    pub name: String,
    /// Where the guest wanted it to run. Kept whether or not it was honoured.
    pub requested_affinity: Affinity,
    /// What that became after the policy applied.
    pub effective_affinity: Affinity,
    /// The priority the guest asked for, recorded and not acted on.
    pub requested_priority: i32,
    /// The scheduling policy the guest asked for, stored verbatim and not interpreted.
    ///
    /// Titles pass vendor values that are not POSIX policy constants; storing them lets
    /// `scePthreadGetschedparam` hand back what was set.
    pub requested_policy: i32,
    /// Whether the guest has asked for cancellation to be disabled on this thread.
    ///
    /// Stored so `pthread_setcancelstate` can hand back the previous value; orbistoun cancels no
    /// threads, so it gates nothing.
    pub cancel_state: i32,
    /// The guest stack it runs on, as the lowest usable address and the length.
    ///
    /// [`None`] for the thread the guest was entered on, whose span the crate root holds, and for a
    /// thread that has not started. Lets `scePthreadAttrGet` answer about a thread by handle, where
    /// [`this_stack`] answers about the caller.
    pub stack: Option<(u64, u64)>,
    /// Whether it has finished.
    pub finished: bool,
}

/// Every guest thread this process has made.
///
/// Global because a handle created on one thread is joined from another.
fn table() -> &'static Mutex<BTreeMap<ThreadHandle, ThreadRecord>> {
    static TABLE: OnceLock<Mutex<BTreeMap<ThreadHandle, ThreadRecord>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// How much zeroed memory sits behind a handle.
///
/// Larger than any field the guest reads at a small offset. The real layout is not known, so
/// every field reads zero, and a guest checking a pointer field for null takes its own error
/// path.
pub const CONTROL_BLOCK_WORDS: usize = 32;

/// Hands out handles: the address of a fresh zeroed block.
///
/// Taken from the one region every guest-visible handle comes from, so handle n is the same
/// address in every run (D584). Never freed: a guest keeping a handle past the thread's life
/// reads zeroes rather than freed memory, and the count is bounded by the threads a title makes.
fn next_handle() -> ThreadHandle {
    let address = orbistoun_mem::blocks::block(CONTROL_BLOCK_WORDS);
    debug_assert_ne!(
        address, NO_THREAD,
        "a live thread must not look like no thread"
    );
    address
}

/// Whether the handles this run issued are the ones it would issue again.
///
/// False for a run that fell back to the host heap, whose addresses look plausible either way.
#[must_use]
pub fn handles_repeat() -> bool {
    orbistoun_mem::blocks::repeat()
}

/// Handles this crate has issued, so a guest-supplied value can be checked before it is used.
fn issued() -> &'static Mutex<std::collections::BTreeSet<ThreadHandle>> {
    static ISSUED: OnceLock<Mutex<std::collections::BTreeSet<ThreadHandle>>> = OnceLock::new();
    ISSUED.get_or_init(|| Mutex::new(std::collections::BTreeSet::new()))
}

/// Whether a value is a handle this crate handed out.
///
/// Handles are addresses, so an arbitrary guest value treated as one would be a write through a
/// bad pointer.
pub fn is_issued(handle: ThreadHandle) -> bool {
    issued().lock().is_ok_and(|i| i.contains(&handle))
}

/// Records a new guest thread and returns its handle.
pub fn register(
    name: &str,
    requested_affinity: Affinity,
    requested_priority: i32,
    policy: AffinityPolicy,
    host_cores: u32,
) -> Option<ThreadHandle> {
    let effective = requested_affinity.mapped(policy, host_cores)?;
    let handle = next_handle();
    if let Ok(mut issued) = issued().lock() {
        issued.insert(handle);
    }
    let record = ThreadRecord {
        handle,
        // Zero until it runs; `become_thread` fills it in on the thread itself.
        host: 0,
        name: name.to_owned(),
        requested_affinity,
        effective_affinity: effective,
        requested_priority,
        requested_policy: 0,
        cancel_state: 0,
        stack: None,
        finished: false,
    };
    table().lock().ok()?.insert(handle, record);
    Some(handle)
}

/// What is known about a thread.
pub fn record(handle: ThreadHandle) -> Option<ThreadRecord> {
    table().lock().ok()?.get(&handle).cloned()
}

/// Records the scheduling a guest asked for, answering whether the thread is known.
///
/// Titles read it back with `scePthreadGetschedparam`, so the setter keeps what it was given.
/// `policy` is [`None`] where the caller set only a priority, so `scePthreadSetprio` does not
/// reset the policy.
pub fn set_scheduling(handle: ThreadHandle, policy: Option<i32>, priority: i32) -> bool {
    let Ok(mut table) = table().lock() else {
        return false;
    };
    let Some(record) = table.get_mut(&handle) else {
        return false;
    };
    if let Some(policy) = policy {
        record.requested_policy = policy;
    }
    record.requested_priority = priority;
    true
}

/// Sets the cancellation state, answering the one it replaced.
///
/// [`None`] where the thread is not one this crate issued, which the caller tells apart from a
/// previous state of zero.
pub fn swap_cancel_state(handle: ThreadHandle, state: i32) -> Option<i32> {
    let mut table = table().lock().ok()?;
    let record = table.get_mut(&handle)?;
    Some(std::mem::replace(&mut record.cancel_state, state))
}

/// Renames a thread, answering whether it is one this crate issued.
///
/// The name is what a trace shows in place of a handle.
pub fn rename(handle: ThreadHandle, name: &str) -> bool {
    let Ok(mut table) = table().lock() else {
        return false;
    };
    let Some(record) = table.get_mut(&handle) else {
        return false;
    };
    name.clone_into(&mut record.name);
    true
}

/// Marks a thread as finished.
pub fn finish(handle: ThreadHandle) {
    if let Ok(mut table) = table().lock() {
        if let Some(record) = table.get_mut(&handle) {
            record.finished = true;
        }
    }
}

/// Every thread, for a report.
pub fn all() -> Vec<ThreadRecord> {
    table()
        .lock()
        .map(|t| t.values().cloned().collect())
        .unwrap_or_default()
}

/// Host threads still running guest code, so they can be joined.
///
/// Separate from the record table because a join consumes the handle and the record must
/// outlive it.
fn joiners() -> &'static Mutex<BTreeMap<ThreadHandle, std::thread::JoinHandle<()>>> {
    static JOINERS: OnceLock<Mutex<BTreeMap<ThreadHandle, std::thread::JoinHandle<()>>>> =
        OnceLock::new();
    JOINERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// What each finished guest thread returned in `rax`, kept so a join can hand it back.
///
/// Separate from `joiners` because the value must be readable after the join handle is
/// consumed. It is stored from inside the thread's body, so it is in place when `join` sees the
/// host thread end.
fn exit_values() -> &'static Mutex<BTreeMap<ThreadHandle, u64>> {
    static EXITS: OnceLock<Mutex<BTreeMap<ThreadHandle, u64>>> = OnceLock::new();
    EXITS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Records what a thread returned, called from inside its body.
fn set_exit_value(handle: ThreadHandle, value: u64) {
    if let Ok(mut map) = exit_values().lock() {
        map.insert(handle, value);
    }
}

/// The value a joined thread returned, or zero if it left none. Consumes the record.
#[must_use]
pub fn exit_value(handle: ThreadHandle) -> u64 {
    exit_values()
        .lock()
        .ok()
        .and_then(|mut m| m.remove(&handle))
        .unwrap_or(0)
}

/// Why a thread could not be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnError {
    /// The affinity request could not be honoured under [`AffinityPolicy::Strict`].
    AffinityUnsatisfiable,
    /// A stack could not be reserved at the address chosen for this thread.
    NoStack,
    /// The host refused to start a thread.
    HostRefused,
}

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AffinityUnsatisfiable => {
                f.write_str("the host cannot provide the cores the guest asked for")
            }
            Self::NoStack => f.write_str("no stack could be reserved for the thread"),
            Self::HostRefused => f.write_str("the host refused to start a thread"),
        }
    }
}

/// What a new guest thread is asked to run.
#[derive(Debug, Clone, Copy)]
pub struct Start {
    /// Guest address of the thread body.
    pub entry: u64,
    /// The single argument it is called with, passed through unexamined.
    pub argument: u64,
}

/// Starts a guest thread on a real host thread.
///
/// Each gets its own stack at its own address and its own thread pointer, the per-thread state
/// guest code reaches without asking.
///
/// # Errors
///
/// When affinity cannot be satisfied, a stack cannot be reserved, or the host refuses.
///
/// # Safety
///
/// `start.entry` must point at mapped, executable, fully relocated guest code, and the
/// thread body will run arbitrary guest instructions on a thread this process owns.
pub unsafe fn spawn(
    start: Start,
    name: &str,
    requested_affinity: Affinity,
    requested_priority: i32,
    requested_stack: u64,
) -> Result<ThreadHandle, SpawnError> {
    // The host's core count, although the guest was told the target's: folding a mask onto cores
    // that do not exist here would place threads nowhere.
    let host_cores = CpuTopology::host().cores;
    let policy = configured().affinity;
    let handle = register(
        name,
        requested_affinity,
        requested_priority,
        policy,
        host_cores,
    )
    .ok_or(SpawnError::AffinityUnsatisfiable)?;

    let slot = next_stack_index();
    let body = move || {
        become_thread(handle);
        let base = stack_base_for(slot);
        // The requested size, capped to what fits this slot: slots are `THREAD_STACK_SPACING` apart,
        // and a reservation adds a guard page below, a read-ahead page above and up to a page of
        // rounding. A larger request gets the largest size that fits.
        let cap = THREAD_STACK_SPACING
            .saturating_sub(orbistoun_mem::stack::GUARD_SIZE)
            .saturating_sub(orbistoun_mem::stack::READAHEAD_GUARD)
            .saturating_sub(orbistoun_mem::stack::GUARD_SIZE);
        let Ok(stack) = orbistoun_mem::stack::GuestStack::reserve(base, requested_stack.min(cap))
        else {
            // A thread that could not get a stack never ran, and the record says so.
            finish(handle);
            return;
        };
        // Published as guest memory, so arguments pointing into the stack dump as stack addresses
        // (D387).
        orbistoun_thunk::note_readable_range(stack.lowest_usable(), stack.len());
        // And to this thread, so `sceKernelIsStack` can answer about it.
        note_this_stack(stack.lowest_usable(), stack.len());
        // Give this thread its own thread-local storage before it runs any guest code. Runs here, on
        // the new thread, because the `fs` base it installs is per-thread. A no-op when nothing
        // installed the hook.
        if let Some(start) = ON_THREAD_START.get() {
            start();
        }
        // The same float environment the process entry adopts. Per thread, because `MXCSR` is a
        // per-thread register and a fresh host thread starts with the host default.
        orbistoun_abi::enter::adopt_guest_float_environment();
        // The value the guest thread function returns in `rax`, kept so a join can hand it back.
        // SAFETY: the caller of `spawn` guarantees `entry` is mapped, executable and
        // relocated; `stack` is a freshly reserved, writable, aligned guest stack with a
        // guard page beneath it, and it outlives the call because it is dropped after.
        let exit = unsafe {
            orbistoun_abi::enter::enter_guest_with_argument(
                start.entry,
                stack.initial_pointer(),
                start.argument,
            )
        };
        set_exit_value(handle, exit);
        finish(handle);
    };

    let spawned = std::thread::Builder::new()
        // Named so a host debugger and a trace agree about which thread is which.
        .name(format!("guest:{name}"))
        .spawn(body)
        .map_err(|_| SpawnError::HostRefused)?;

    if let Ok(mut joiners) = joiners().lock() {
        joiners.insert(handle, spawned);
    }
    Ok(handle)
}

/// The arena reentrant guest calls take their stacks from.
///
/// Distinct from the thread stacks ([`THREAD_STACK_BASE`]) and the mapping arena. The addresses
/// are never handed to the guest as data.
const REENTRANT_STACK_BASE: u64 = 0x0000_6800_0000_0000;

/// Calls a guest function synchronously, on a fresh stack, and returns what it left in `rax`.
///
/// Some calls take callbacks (a `call_once` initialiser, an `atexit` handler, a sort
/// comparator), so a handler in flight calls back into guest code and continues. The callback
/// runs on the same thread, keeping its thread-local state, but on a stack of its own so it
/// cannot overwrite the handler's frames. Up to three arguments.
///
/// # Safety
///
/// `entry` must point at mapped, executable, relocated guest code following System V - a function
/// pointer the guest itself handed over. The arguments are passed unexamined, so a dereferenced one
/// must be a valid guest address.
pub unsafe fn call_guest(entry: u64, args: [u64; 3]) -> Option<u64> {
    // SAFETY: forwarded unchanged to the general form, whose contract this one's repeats.
    unsafe { call_guest_placing(entry, |_, _| args) }
}

/// [`call_guest`], with the arguments chosen once the stack exists.
///
/// A signal handler is handed a pointer to a context structure on the stack it runs on, and a
/// guest may scan from it to the end of that allocation. Only this function knows where the
/// stack is, so `place` receives its lowest usable address and length and answers the three
/// arguments.
///
/// # Safety
///
/// As [`call_guest`]. Anything `place` writes must stay within the span it is given.
pub unsafe fn call_guest_placing(
    entry: u64,
    place: impl FnOnce(u64, u64) -> [u64; 3],
) -> Option<u64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(REENTRANT_STACK_BASE);
    // One stack plus a gap, so nested reentrant calls never share a stack. The base only advances.
    let step = orbistoun_mem::stack::DEFAULT_STACK_SIZE.saturating_mul(2);
    let base = NEXT.fetch_add(step, Ordering::Relaxed);
    let stack =
        orbistoun_mem::stack::GuestStack::reserve(base, orbistoun_mem::stack::DEFAULT_STACK_SIZE)
            .ok()?;
    // So a callback's own arguments dump as stack addresses (D387).
    orbistoun_thunk::note_readable_range(stack.lowest_usable(), stack.len());
    let args = place(stack.lowest_usable(), stack.len());
    // SAFETY: the caller vouches `entry` is executable guest code; `stack` is a fresh, aligned,
    // guarded guest stack that outlives the call - dropped below, after it returns.
    let result = unsafe {
        orbistoun_abi::enter::enter_guest_with_three_arguments(
            entry,
            stack.initial_pointer(),
            args[0],
            args[1],
            args[2],
        )
    };
    Some(result)
}

/// Waits for a guest thread to finish.
///
/// Returns whether there was anything to wait for. A second join on the same handle answers
/// `false` rather than blocking forever.
pub fn join(handle: ThreadHandle) -> bool {
    let taken = joiners().lock().ok().and_then(|mut j| j.remove(&handle));
    match taken {
        Some(thread) => {
            // Discarded: a guest thread that panicked has already been reported by the fault reporter.
            let _ = thread.join();
            true
        }
        None => false,
    }
}

/// The handle of the thread this code is running on.
///
/// A thread-local, because `scePthreadSelf` is asked constantly and the answer is fixed for the
/// thread's life. Zero on a thread the guest did not create.
fn current_handle() -> &'static std::thread::LocalKey<std::cell::Cell<ThreadHandle>> {
    thread_local! {
        static CURRENT: std::cell::Cell<ThreadHandle> = const { std::cell::Cell::new(NO_THREAD) };
    }
    &CURRENT
}

/// Which guest thread is running here.
pub fn current() -> ThreadHandle {
    current_handle().with(std::cell::Cell::get)
}

/// Claims this host thread as a given guest thread.
///
/// Called once, at the top of a spawned thread, before any guest code runs on it.
pub fn become_thread(handle: ThreadHandle) {
    current_handle().with(|c| c.set(handle));
    // Recorded here rather than at registration: a thread is created by one thread and runs on
    // another.
    if let Ok(mut table) = table().lock() {
        if let Some(record) = table.get_mut(&handle) {
            record.host = orbistoun_thunk::host_thread();
        }
    }
    // The signal slot is cached in a thread-local so a wait predicate reads it without a lock
    // (D652).
    if let Ok(mut slots) = signal_slots().lock() {
        let slot = Arc::clone(slots.entry(handle).or_default());
        MY_SIGNALS.with(|s| *s.borrow_mut() = Some(slot));
    }
}

/// One thread's signal state: what has been raised on it, and whether a raise can reach it.
///
/// An `Arc` of atomics rather than a field in [`ThreadRecord`]: the pending flag is read inside
/// a condition-variable predicate under the wait queue's lock, and `sceKernelRaiseException`
/// takes the table and queue locks in the other order. A slot read with no lock avoids the
/// inversion (D652).
#[derive(Debug, Default)]
pub struct SignalSlot {
    /// The signal number raised and not yet run, or zero for none.
    pending: AtomicU64,
    /// Whether this thread is inside a wait that consults the pending flag.
    ///
    /// A thread spinning in guest code, or blocked in a wait that does not check, is not parked; a
    /// signal raised on a parked thread runs.
    parked: AtomicBool,
}

/// Every thread's signal slot, by handle.
fn signal_slots() -> &'static Mutex<BTreeMap<ThreadHandle, Arc<SignalSlot>>> {
    static SLOTS: OnceLock<Mutex<BTreeMap<ThreadHandle, Arc<SignalSlot>>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

thread_local! {
    /// This thread's own slot, so reading it needs no lock.
    ///
    /// Filled by [`become_thread`], which runs on the thread itself and holds no queue lock.
    static MY_SIGNALS: std::cell::RefCell<Option<Arc<SignalSlot>>> =
        const { std::cell::RefCell::new(None) };
}

/// This thread's slot, if it has been adopted.
fn my_slot() -> Option<Arc<SignalSlot>> {
    MY_SIGNALS.with(|s| s.borrow().clone())
}

/// Marks `signum` as raised on `handle`, answering whether the thread can be reached.
///
/// `false` when the thread is not parked in a wait that consults its slot, so the caller
/// refuses rather than answering a success the handler would never honour (D652).
pub fn raise_pending(handle: ThreadHandle, signum: u64) -> bool {
    let Ok(slots) = signal_slots().lock() else {
        return false;
    };
    let Some(slot) = slots.get(&handle) else {
        return false;
    };
    if !slot.parked.load(Ordering::Acquire) {
        return false;
    }
    slot.pending.store(signum, Ordering::Release);
    true
}

/// Whether a signal is waiting to run on this thread. Lock-free, for a wait predicate.
#[must_use]
pub fn signal_pending() -> bool {
    my_slot().is_some_and(|slot| slot.pending.load(Ordering::Acquire) != 0)
}

/// Takes the signal waiting on this thread, leaving none.
pub fn take_pending() -> Option<u64> {
    let slot = my_slot()?;
    match slot.pending.swap(0, Ordering::AcqRel) {
        0 => None,
        signum => Some(signum),
    }
}

/// Records that this thread is, or is no longer, inside a wait that consults its slot.
///
/// Returns the previous value so a nested wait restores rather than clears it.
pub fn set_parked(parked: bool) -> bool {
    my_slot().is_some_and(|slot| slot.parked.swap(parked, Ordering::AcqRel))
}

/// A hook run at the top of every spawned guest thread, before it enters guest code.
///
/// The layer that builds the guest's thread-local storage installs it: the loader parses the
/// TLS template and the worker holds it, so this crate spawns the thread without owning the
/// setup. Without a hook a spawned thread runs with no TLS block.
static ON_THREAD_START: OnceLock<fn()> = OnceLock::new();

/// Installs the per-thread start hook. Called once, by the worker, before the guest is entered.
pub fn install_thread_start(hook: fn()) {
    let _ = ON_THREAD_START.set(hook);
}

/// Gives the calling host thread a handle if it does not already have one.
///
/// The process's first thread runs guest code without being created by the guest, and the guest
/// asks it who it is. Answering zero would make every unadopted thread equal to every other.
pub fn adopt(name: &str) -> ThreadHandle {
    let existing = current();
    if existing != NO_THREAD {
        return existing;
    }
    let host_cores = CpuTopology::host().cores;
    // Observe rather than the configured policy: this thread is already placed.
    let handle = register(
        name,
        Affinity::default(),
        0,
        AffinityPolicy::Observe,
        host_cores,
    )
    .unwrap_or(NO_THREAD);
    become_thread(handle);
    handle
}

thread_local! {
    /// The guest stack this host thread is running on, if it is running one.
    ///
    /// A thread-local, because `sceKernelIsStack` asks about the calling thread; a table of every
    /// stack would answer yes for another thread's.
    static MY_STACK: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
}

/// Records the guest stack this thread runs on.
fn note_this_stack(base: u64, len: u64) {
    MY_STACK.with(|held| held.set(Some((base, len))));
    // And into the record, so the question can be asked about this thread by handle.
    if let Ok(mut table) = table().lock()
        && let Some(record) = table.get_mut(&current())
    {
        record.stack = Some((base, len));
    }
}

/// The guest stack a thread runs on, by handle: the lowest usable address and the length.
///
/// [`None`] for a handle nobody registered, for a thread that has not reserved its stack, and
/// for the thread the guest was entered on, whose span belongs to the crate root.
#[must_use]
pub fn stack_of(handle: ThreadHandle) -> Option<(u64, u64)> {
    table().lock().ok()?.get(&handle)?.stack
}

/// The guest stack this thread runs on, if it is a guest thread.
///
/// [`None`] on the thread the guest was entered on, which uses the main span instead, and on
/// any thread of this emulator's own.
#[must_use]
pub fn this_stack() -> Option<(u64, u64)> {
    MY_STACK.with(std::cell::Cell::get)
}

/// Where guest thread stacks are reserved.
///
/// Stacks are spaced by more than the largest stack, so a guard page always has unmapped space
/// beneath it and an overrun faults rather than landing in another stack.
pub const THREAD_STACK_BASE: u64 = 0x0000_6100_0000_0000;
/// Distance between one thread's stack and the next.
pub const THREAD_STACK_SPACING: u64 = 64 * 1024 * 1024;

/// The stack address for the `nth` guest thread.
pub const fn stack_base_for(index: u64) -> u64 {
    THREAD_STACK_BASE.wrapping_add(index.wrapping_mul(THREAD_STACK_SPACING))
}

/// The next stack slot.
///
/// A counter of its own, since handles are addresses. Never reused, so a stack cannot go to a
/// second thread while the first is still unwinding out of it.
fn next_stack_index() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {

    /// A thread that is not parked refuses the signal.
    ///
    /// An accepted raise tells the guest the handler will run, which only a thread in a wait that
    /// consults its slot can do (D652).
    #[test]
    fn a_raise_on_a_thread_that_is_not_parked_is_refused() {
        let handle = super::adopt("not-parked");
        assert!(handle != NO_THREAD, "the thread has a handle");

        assert!(
            !super::raise_pending(handle, 30),
            "a thread that is awake cannot be reached, and says so"
        );
        assert!(
            !super::signal_pending(),
            "and nothing was left pending to surprise a later wait"
        );
    }

    /// A raise on a parked thread is accepted, delivered once, and leaves nothing behind.
    #[test]
    fn a_raise_on_a_parked_thread_is_taken_exactly_once() {
        let handle = super::adopt("parked");
        let was = super::set_parked(true);

        assert!(
            super::raise_pending(handle, 30),
            "a parked thread is reachable"
        );
        assert!(super::signal_pending(), "and the wait predicate can see it");
        assert_eq!(
            super::take_pending(),
            Some(30),
            "the signal number survives"
        );
        assert_eq!(
            super::take_pending(),
            None,
            "and taking it twice does not run a handler twice"
        );
        assert!(!super::signal_pending());

        super::set_parked(was);
    }

    /// A raise naming a thread nothing issued is refused rather than remembered.
    #[test]
    fn a_raise_on_an_unknown_handle_is_refused() {
        assert!(
            !super::raise_pending(0xdead_beef, 30),
            "a handle this crate never handed out has no slot to write into"
        );
    }
    /// A thread that is not running a guest stack says so, rather than reporting another thread's.
    #[test]
    fn a_host_thread_has_no_guest_stack() {
        assert_eq!(super::this_stack(), None);
    }

    /// What a thread records is its own, and does not leak to another.
    #[test]
    fn a_recorded_stack_belongs_to_the_thread_that_recorded_it() {
        super::note_this_stack(0x1000, 0x100);
        assert_eq!(super::this_stack(), Some((0x1000, 0x100)));
        let elsewhere = std::thread::spawn(super::this_stack).join().expect("joins");
        assert_eq!(elsewhere, None, "another thread sees nothing of it");
    }

    use super::{Affinity, AffinityPolicy, CpuTopology, NO_THREAD, register};

    #[test]
    fn the_default_topology_is_the_targets_and_not_the_hosts() {
        // A guest asking how many cores it has is asking about the machine it was written for.
        let target = CpuTopology::default();
        assert_eq!(target.cores, 8);
        assert!(
            target.usable < target.cores,
            "the system keeps some for itself"
        );
    }

    #[test]
    fn a_handle_is_never_the_no_thread_value() {
        // Zero is what a caller tests for; a real thread with that handle would read as a failed
        // creation.
        let handle = register("worker", Affinity(0), 0, AffinityPolicy::Observe, 8)
            .expect("observe never refuses");
        assert_ne!(handle, NO_THREAD);
    }

    #[test]
    fn the_requested_mask_is_kept_even_when_it_is_not_honoured() {
        // The default policy records the request, so a title that depends on placement can be found
        // (D150).
        let handle = register(
            "audio",
            Affinity(0b1011_0000),
            0,
            AffinityPolicy::Observe,
            4,
        )
        .expect("registered");
        let record = super::record(handle).expect("present");
        assert_eq!(record.requested_affinity, Affinity(0b1011_0000));
        assert!(
            record.effective_affinity.is_unset(),
            "observe places nothing"
        );
    }

    #[test]
    fn mapping_folds_rather_than_clamps() {
        // Clamping would collapse every out-of-range core onto the highest one.
        let asked = Affinity(0b0011_0000); // cores 4 and 5
        let mapped = asked
            .mapped(AffinityPolicy::Map, 4)
            .expect("map never refuses");
        assert_eq!(mapped, Affinity(0b0011), "4 and 5 fold to 0 and 1");
        assert_eq!(
            mapped.cores().count(),
            2,
            "two distinct cores must stay two"
        );
    }

    #[test]
    fn strict_refuses_what_the_host_cannot_satisfy() {
        // Strict answers whether a title needs exactly what it asked for, by failing when it cannot.
        let asked = Affinity(1 << 6);
        assert!(asked.mapped(AffinityPolicy::Strict, 4).is_none());
        assert_eq!(asked.mapped(AffinityPolicy::Strict, 8), Some(asked));
    }

    #[test]
    fn an_empty_mask_means_anywhere_under_every_policy() {
        // An unset mask is a guest saying it does not care, not a request the host cannot meet.
        for policy in [
            AffinityPolicy::Observe,
            AffinityPolicy::Map,
            AffinityPolicy::Strict,
        ] {
            assert_eq!(
                Affinity(0).mapped(policy, 4),
                Some(Affinity(0)),
                "{policy:?} should accept an unset mask"
            );
        }
    }

    #[test]
    fn settings_survive_a_round_trip_through_a_file() {
        // The settings round-trip through TOML, so they can be edited without a rebuild.
        let chosen = super::Settings {
            topology: CpuTopology {
                cores: 4,
                usable: 3,
            },
            affinity: AffinityPolicy::Map,
            apply_priority: true,
        };
        let text = toml::to_string(&chosen).expect("settings serialise");
        let back: super::Settings = toml::from_str(&text).expect("and read back");
        assert_eq!(back, chosen);
    }

    #[test]
    fn an_empty_configuration_file_is_the_default_rather_than_an_error() {
        // Every field is optional, so a file can set a single one.
        let back: super::Settings = toml::from_str("").expect("an empty file is valid");
        assert_eq!(back, super::Settings::default());
    }

    #[test]
    fn a_handle_is_memory_the_guest_can_read_through() {
        // An error code handed back as a thread handle is dereferenced by the guest, so a handle is a
        // real readable block (D151).
        let handle = register(
            "readable",
            Affinity::default(),
            0,
            AffinityPolicy::Observe,
            4,
        )
        .expect("registers");

        assert_ne!(handle, NO_THREAD);
        assert_eq!(handle % 8, 0, "aligned, so a word read is a word read");
        assert!(super::is_issued(handle), "and recognised as one of ours");

        // SAFETY: the address of a leaked, zeroed, aligned block this module owns and
        // never frees, so a word read from it is always valid.
        let first_word = unsafe { std::ptr::read(handle as usize as *const u64) };
        assert_eq!(first_word, 0, "unknown fields read as zero, not as garbage");
    }

    #[test]
    fn an_arbitrary_guest_value_is_not_treated_as_a_handle() {
        // Handles are addresses, so a made-up guest value must not be believed.
        assert!(!super::is_issued(0x1234_5678));
        assert!(!super::is_issued(NO_THREAD));
    }

    #[test]
    fn a_host_thread_the_guest_did_not_make_reports_no_thread() {
        // The process's first thread is unadopted here, and a fabricated handle would be one nothing
        // can join.
        assert_eq!(super::current(), NO_THREAD);
    }

    #[test]
    fn claiming_a_thread_is_visible_only_on_that_thread() {
        // `scePthreadSelf` answers per thread.
        super::become_thread(42);
        assert_eq!(super::current(), 42);

        let other = std::thread::spawn(super::current).join().expect("joined");
        assert_eq!(other, NO_THREAD, "a fresh thread inherits nothing");
        super::become_thread(NO_THREAD);
    }

    #[test]
    fn thread_stacks_are_spaced_further_apart_than_they_are_tall() {
        // Adjacent stacks would let an overrun on one land in the next.
        let gap = super::stack_base_for(1) - super::stack_base_for(0);
        assert!(
            gap > orbistoun_mem::stack::DEFAULT_STACK_SIZE,
            "a stack must not be able to reach its neighbour"
        );
    }

    #[test]
    fn the_default_policy_records_rather_than_places() {
        // Observe is the default: no title examined depends on placement.
        assert_eq!(AffinityPolicy::default(), AffinityPolicy::Observe);
    }
}
