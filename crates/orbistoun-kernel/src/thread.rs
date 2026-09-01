//! Guest threads.
//!
//! A guest thread is a **real host thread**, always. Principle 6 states it and the
//! reasons are not negotiable: guest code reads thread-local storage through the segment
//! base directly, and blocks inside its own synchronisation primitives. A green-threaded
//! or pooled implementation cannot present either of those honestly, and the failures
//! would appear as data corruption rather than as a scheduling bug.
//!
//! # Threads are the guest's decision, not the machine's
//!
//! Worth stating because it is easy to get backwards: the *guest* decides how many
//! threads exist, by asking for them. The host's core count decides how many run at one
//! instant. A title asking for thirty threads gets thirty host threads on a four-core
//! machine and on a thirty-two-core one; the second is faster, not more parallel in any
//! sense the guest can observe.
//!
//! So there is no minimum core count to enforce and nothing to refuse. A slower machine
//! runs the same program more slowly, which is the correct behaviour.
//!
//! # What the host's shape does change
//!
//! Two things, and both are handled deliberately rather than by accident.
//!
//! **What a guest is told about the machine.** A title asking how many cores it has
//! wants the number its designers assumed. Answering with the host's is how a program
//! built for a known machine ends up sizing a thread pool for a machine nobody tested it
//! on - so [`CpuTopology`] reports the target's shape by default, and the host's only if
//! somebody asks for that deliberately.
//!
//! **Affinity.** The guest can pin a thread to particular cores. Honouring a mask
//! literally breaks the moment the host has fewer cores than it names; ignoring it
//! silently discards something the guest thought it was told. Neither is acceptable, so
//! a request is *mapped* and the original is *kept* - see [`AffinityPolicy`] (D150).

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// How the guest sees a thread.
///
/// **An address, because the guest dereferences it.** The first version of this was a
/// small opaque integer, on the reasoning that a handle is passed back rather than read
/// through - and the knowledge file already contained the evidence against that: an
/// unimplemented `scePthreadSelf` returned the error code `0x7FFF0001` and a title
/// faulted with `read of 0x5`, which is that code being dereferenced with an offset.
///
/// A handle of `1` would have reproduced the same fault at a lower address. So each
/// handle is the address of a real, zeroed block this crate owns (D151).
pub type ThreadHandle = u64;

/// Handle given out when a thread could not be created.
///
/// Zero, because that is what a caller tests for.
pub const NO_THREAD: ThreadHandle = 0;

/// The shape of the machine a guest believes it is running on.
///
/// Configurable rather than fixed, because both answers are right for different
/// questions and neither is right for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CpuTopology {
    /// Cores the guest is told exist.
    pub cores: u32,
    /// Cores the guest is told it may actually use.
    ///
    /// Lower than `cores` on the target: the system keeps some for itself, and a title
    /// that spreads work across every core it can see would be competing with the
    /// operating system for the ones it cannot have.
    pub usable: u32,
}

impl Default for CpuTopology {
    fn default() -> Self {
        // The target's shape, not the host's. A guest asking this question is asking
        // about the machine it was written for, and answering with a thirty-two-core
        // host is how a program sizes a pool nobody ever tested.
        //
        // A stated assumption: nothing here has measured the real figures, and a title
        // that behaves differently when they change will do so silently.
        Self {
            cores: 8,
            usable: 7,
        }
    }
}

impl CpuTopology {
    /// What the machine orbistoun is running on actually has.
    ///
    /// Offered so it can be chosen deliberately - a developer measuring throughput wants
    /// the truth, and a guest almost never does.
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
/// **The request is never silently discarded.** Whichever policy applies, the mask the
/// guest asked for is recorded on the thread, so a title that turns out to depend on
/// affinity can be found rather than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AffinityPolicy {
    /// Record the request and let the host scheduler place the thread.
    ///
    /// The default, and deliberately not because it is easiest. No title examined has
    /// been shown to depend on placement, and a mapping invented before there is
    /// evidence is a guess that later looks like a measurement. Requests are recorded
    /// precisely so that evidence can appear.
    #[default]
    Observe,
    /// Fold the guest's mask onto the host's cores.
    ///
    /// For when a title is shown to care. Guest core `n` becomes host core `n % host`,
    /// which preserves *distinctness* - two guest threads pinned apart stay apart - and
    /// gives up exact placement, which is not reproducible across machines anyway.
    Map,
    /// Apply the mask as given, and fail if the host cannot satisfy it.
    ///
    /// Never a default. It is here because "did this title need exactly what it asked
    /// for?" is a question worth being able to answer.
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
    /// `None` means the request cannot be honoured and the caller should refuse - only
    /// ever under [`AffinityPolicy::Strict`].
    pub fn mapped(self, policy: AffinityPolicy, host_cores: u32) -> Option<Self> {
        if self.is_unset() || host_cores == 0 {
            return Some(Self(0));
        }
        match policy {
            AffinityPolicy::Observe => Some(Self(0)),
            AffinityPolicy::Map => {
                // Fold rather than clamp. Clamping collapses every out-of-range core onto
                // the highest one, which silently puts threads the guest deliberately
                // separated back together.
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
/// In one struct, serialisable, because principle 5 says rules live in data: answering
/// "how many cores does the guest think it has?" must be a file edit and a relaunch, not
/// a rebuild. The bisection loop is the only oracle most of this project has, and
/// anything requiring a recompile to try is effectively untriable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    /// The machine the guest is told about.
    pub topology: CpuTopology,
    /// What happens to an affinity request.
    pub affinity: AffinityPolicy,
    /// Whether the guest's priority requests are applied to host threads.
    ///
    /// Off by default and not yet implemented either way: raising a host thread's
    /// priority on the strength of a guest's opinion can starve the emulator's own
    /// threads, and no title has been shown to need it.
    pub apply_priority: bool,
}

/// The settings in force.
fn settings() -> &'static Mutex<Settings> {
    static SETTINGS: OnceLock<Mutex<Settings>> = OnceLock::new();
    SETTINGS.get_or_init(|| Mutex::new(Settings::default()))
}

/// Replaces the threading settings.
///
/// Called once during setup. Later threads see the new settings; threads already running
/// keep whatever they were placed under, because moving a running thread would change
/// the program mid-flight and make a trace impossible to read.
pub fn configure(new: Settings) {
    if let Ok(mut current) = settings().lock() {
        *current = new;
    }
}

/// The settings in force right now.
pub fn configured() -> Settings {
    settings().lock().map(|s| *s).unwrap_or_default()
}

/// One guest thread, and what was asked for when it was made.
#[derive(Debug, Clone)]
pub struct ThreadRecord {
    /// The handle the guest holds.
    pub handle: ThreadHandle,
    /// The name the guest gave it, if any. Traces are far more readable with it.
    pub name: String,
    /// Where the guest wanted it to run. Kept whether or not it was honoured.
    pub requested_affinity: Affinity,
    /// What that became after the policy applied.
    pub effective_affinity: Affinity,
    /// The priority the guest asked for, recorded and not yet acted on.
    pub requested_priority: i32,
    /// Whether it has finished.
    pub finished: bool,
}

/// Every guest thread this process has made.
///
/// Global because the guest's own model is: a handle created on one thread is joined
/// from another, and both must see the same table.
fn table() -> &'static Mutex<BTreeMap<ThreadHandle, ThreadRecord>> {
    static TABLE: OnceLock<Mutex<BTreeMap<ThreadHandle, ThreadRecord>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// How much zeroed memory sits behind a handle.
///
/// Larger than any plausible field the guest reads at a small offset, and cheap. The
/// real structure's layout is not known from any lawful source, so nothing is written
/// into it - **every field the guest reads is zero**, which for a pointer field means
/// null, and a guest that checks for null takes its own error path rather than
/// dereferencing garbage.
pub const CONTROL_BLOCK_WORDS: usize = 32;

/// Hands out handles: the address of a fresh zeroed block.
///
/// The blocks are deliberately **never freed**. A guest keeping a handle past the
/// thread's life is then a read of zeroes rather than a use-after-free, and the count is
/// bounded by how many threads a title makes.
fn next_handle() -> ThreadHandle {
    let block: Box<[u64; CONTROL_BLOCK_WORDS]> = Box::new([0; CONTROL_BLOCK_WORDS]);
    // Eight-byte aligned by construction, so a guest reading a word out of it does an
    // aligned read - which a `Vec<u8>` would not have guaranteed.
    let address = std::ptr::from_mut(Box::leak(block)) as usize as u64;
    debug_assert_ne!(
        address, NO_THREAD,
        "a live thread must not look like no thread"
    );
    address
}

/// Handles this crate has issued, so a guest-supplied value can be checked before it is
/// believed.
fn issued() -> &'static Mutex<std::collections::BTreeSet<ThreadHandle>> {
    static ISSUED: OnceLock<Mutex<std::collections::BTreeSet<ThreadHandle>>> = OnceLock::new();
    ISSUED.get_or_init(|| Mutex::new(std::collections::BTreeSet::new()))
}

/// Whether a value is a handle this crate actually handed out.
///
/// Necessary now that handles are addresses: an arbitrary guest value must never be
/// treated as one, or a bad pointer from the guest becomes a write through it here.
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
        name: name.to_owned(),
        requested_affinity,
        effective_affinity: effective,
        requested_priority,
        finished: false,
    };
    table().lock().ok()?.insert(handle, record);
    Some(handle)
}

/// What is known about a thread.
pub fn record(handle: ThreadHandle) -> Option<ThreadRecord> {
    table().lock().ok()?.get(&handle).cloned()
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
/// Separate from the record table because a join *consumes* the handle, and the record
/// must outlive it - a guest that joins a thread and then asks its name should still get
/// an answer, and a trace of a finished thread is more useful than a gap.
fn joiners() -> &'static Mutex<BTreeMap<ThreadHandle, std::thread::JoinHandle<()>>> {
    static JOINERS: OnceLock<Mutex<BTreeMap<ThreadHandle, std::thread::JoinHandle<()>>>> =
        OnceLock::new();
    JOINERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// What each finished guest thread returned - `rax` when its body returned - kept so a join can
/// hand it back. Separate from `joiners` because the value has to be readable *after* the join
/// handle is consumed, and it is stored from inside the thread's own body once its guest function
/// has returned, so it is in place by the time `join` sees the host thread end (030-thread/join).
fn exit_values() -> &'static Mutex<BTreeMap<ThreadHandle, u64>> {
    static EXITS: OnceLock<Mutex<BTreeMap<ThreadHandle, u64>>> = OnceLock::new();
    EXITS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Record what a thread returned, called from inside its body.
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
/// Each one gets its own stack at its own address, and its own thread pointer - the two
/// pieces of per-thread state guest code reaches without asking. Sharing either would
/// look like memory corruption rather than like a threading bug (principle 6).
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
) -> Result<ThreadHandle, SpawnError> {
    // The *host's* core count, deliberately, even though the guest was told the target's.
    // Folding a mask onto cores that do not exist here would place threads nowhere.
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
        let Ok(stack) = orbistoun_mem::stack::GuestStack::reserve(
            base,
            orbistoun_mem::stack::DEFAULT_STACK_SIZE,
        ) else {
            // Reported rather than panicked: a thread that could not get a stack is a
            // thread that never ran, and the record says so.
            finish(handle);
            return;
        };
        // **Published as guest memory, because that is what it is.** The readable ranges are
        // installed before the guest is entered, when no thread stack exists yet - so every
        // argument a thread passed dumped as `no region this run mapped`, which reads as a
        // wild pointer and is an ordinary stack address (D387).
        orbistoun_thunk::note_readable_range(stack.lowest_usable(), stack.len());
        // And to this thread, so `sceKernelIsStack` can answer about it. Same fact, needed by
        // a second subsystem that had the same blind spot (D391).
        note_this_stack(stack.lowest_usable(), stack.len());
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
/// Distinct from the thread stacks ([`THREAD_STACK_BASE`]) and the mapping arena, so a reentrant
/// call's stack cannot collide with a thread's or a guest allocation's. The addresses here are
/// never handed to the guest as data.
const REENTRANT_STACK_BASE: u64 = 0x0000_6800_0000_0000;

/// Calls a guest function synchronously, on a fresh stack, and returns what it left in `rax`.
///
/// # Why an HLE call needs this
///
/// Some calls a guest makes *are* callbacks: `call_once`'s initialiser, an `atexit` handler, a
/// comparator handed to a sort. Implementing one means calling back into guest code from inside a
/// handler already in flight, and then continuing - the same transfer [`spawn`] does for a new
/// thread, but nested on the current one rather than forwards on a fresh one.
///
/// **A fresh stack, on the same thread.** The caller is mid-handler on the guest thread's own stack;
/// a callback grown down into it would overwrite frames the handler still needs. A stack of its own
/// avoids that while keeping the thread's thread-local state, which is what a callback usually
/// reads. It is released when this returns. Up to three arguments - what an `InitOnce`-shaped
/// callback takes.
///
/// # Safety
///
/// `entry` must point at mapped, executable, relocated guest code following System V - a function
/// pointer the guest itself handed over. The arguments are passed unexamined, so a dereferenced one
/// must be a valid guest address.
pub unsafe fn call_guest(entry: u64, args: [u64; 3]) -> Option<u64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(REENTRANT_STACK_BASE);
    // One stack plus a gap, so nested reentrant calls never share a stack; the base advances and is
    // not reused, which a run makes far too few reentrant calls to exhaust the arena of.
    let step = orbistoun_mem::stack::DEFAULT_STACK_SIZE.saturating_mul(2);
    let base = NEXT.fetch_add(step, Ordering::Relaxed);
    let stack =
        orbistoun_mem::stack::GuestStack::reserve(base, orbistoun_mem::stack::DEFAULT_STACK_SIZE)
            .ok()?;
    // So a callback's own arguments dump as stack addresses rather than wild pointers (D387).
    orbistoun_thunk::note_readable_range(stack.lowest_usable(), stack.len());
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
/// Returns whether there was anything to wait for. A second join on the same handle
/// answers `false` rather than blocking forever, which is what a double-join is.
pub fn join(handle: ThreadHandle) -> bool {
    let taken = joiners().lock().ok().and_then(|mut j| j.remove(&handle));
    match taken {
        Some(thread) => {
            // The result is discarded deliberately: a guest thread that panicked has
            // already been reported by the fault reporter, and there is nothing useful
            // to hand back to a guest that only asked whether it was done.
            let _ = thread.join();
            true
        }
        None => false,
    }
}

/// The handle of the thread this code is running on.
///
/// A thread-local rather than a lookup, because `scePthreadSelf` is asked constantly and
/// the answer is fixed for the life of the thread. Zero on a thread the guest did not
/// create - the process's first thread among them, which the guest also asks about.
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
}

/// Gives the calling host thread a handle if it does not already have one.
///
/// The process's first thread runs guest code without ever having been created by the
/// guest, and the guest still asks it who it is. Answering zero would be answering "no
/// thread" about a thread that is demonstrably running, and a guest comparing thread
/// identities would find every unadopted thread equal to every other.
pub fn adopt(name: &str) -> ThreadHandle {
    let existing = current();
    if existing != NO_THREAD {
        return existing;
    }
    let host_cores = CpuTopology::host().cores;
    // Observe rather than the configured policy: this thread is already placed, and
    // nothing was requested for it.
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
    /// A thread-local rather than a registry, because the question `sceKernelIsStack` asks is
    /// about **the calling thread**: a table of every guest stack would answer yes for another
    /// thread's, which is a different question with a different right answer.
    static MY_STACK: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
}

/// Records the guest stack this thread runs on.
fn note_this_stack(base: u64, len: u64) {
    MY_STACK.with(|held| held.set(Some((base, len))));
}

/// The guest stack this thread runs on, if it is a guest thread.
///
/// [`None`] on the thread the guest was entered on, which uses the main span instead, and on
/// any thread of this emulator's own.
#[must_use]
pub fn this_stack() -> Option<(u64, u64)> {
    MY_STACK.with(std::cell::Cell::get)
}

/// Where guest thread stacks are reserved, and how far apart.
///
/// Spaced by more than the largest stack so a guard page always has unmapped space
/// beneath it - two stacks packed adjacently would let an overrun on one land in the
/// other, which reads as memory corruption rather than as a stack overflow.
pub const THREAD_STACK_BASE: u64 = 0x0000_6100_0000_0000;
/// Distance between one thread's stack and the next.
pub const THREAD_STACK_SPACING: u64 = 64 * 1024 * 1024;

/// The stack address for the `nth` guest thread.
pub const fn stack_base_for(index: u64) -> u64 {
    THREAD_STACK_BASE.wrapping_add(index.wrapping_mul(THREAD_STACK_SPACING))
}

/// The next stack slot.
///
/// A counter of its own rather than anything derived from the handle: handles became
/// host addresses, and multiplying one by the stack spacing lands somewhere arbitrary.
/// Never reused, so a stack cannot be handed to a second thread while the first is
/// still unwinding out of it.
fn next_stack_index() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    /// **A thread that is not running a guest stack says so**, rather than claiming one.
    ///
    /// The failure this protects against is the silent direction: `sceKernelIsStack` falling
    /// back to whatever the last thread recorded would answer yes about another thread's
    /// stack, which is a different question (D391).
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
        // A guest asking how many cores it has is asking about the machine it was
        // written for. Answering with a thirty-two-core host is how a program sizes a
        // thread pool for a machine nobody tested it on.
        let target = CpuTopology::default();
        assert_eq!(target.cores, 8);
        assert!(
            target.usable < target.cores,
            "the system keeps some for itself"
        );
    }

    #[test]
    fn a_handle_is_never_the_no_thread_value() {
        // Zero is what a caller tests for. A real thread colliding with it would read as
        // a failed creation that then gets joined.
        let handle = register("worker", Affinity(0), 0, AffinityPolicy::Observe, 8)
            .expect("observe never refuses");
        assert_ne!(handle, NO_THREAD);
    }

    #[test]
    fn the_requested_mask_is_kept_even_when_it_is_not_honoured() {
        // The whole point of the default policy. A title that turns out to depend on
        // placement has to be findable, and it cannot be if the request was discarded
        // at the door (D150).
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
        // Clamping collapses every out-of-range core onto the highest one, silently
        // putting threads the guest deliberately separated back together.
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
        // The reason this policy exists: it answers "did this title need exactly what it
        // asked for?" and it can only answer that by failing when it cannot.
        let asked = Affinity(1 << 6);
        assert!(asked.mapped(AffinityPolicy::Strict, 4).is_none());
        assert_eq!(asked.mapped(AffinityPolicy::Strict, 8), Some(asked));
    }

    #[test]
    fn an_empty_mask_means_anywhere_under_every_policy() {
        // Not a request the host cannot meet - a guest saying it does not care.
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
        // The whole point of putting these in data: if they cannot be written and read
        // back, "edit a TOML and relaunch" is not actually available and every question
        // about threading costs a rebuild.
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
        // A configuration that must be complete to be valid is one nobody can edit a
        // single field of.
        let back: super::Settings = toml::from_str("").expect("an empty file is valid");
        assert_eq!(back, super::Settings::default());
    }

    #[test]
    fn a_handle_is_memory_the_guest_can_read_through() {
        // The evidence for this was sitting in the knowledge file before the code was
        // written: an unimplemented `scePthreadSelf` returned `0x7FFF0001` and a title
        // faulted with `read of 0x5` - the error code being dereferenced at an offset.
        // A handle of `1` reproduces that fault at a lower address (D151).
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
        // Now that handles are addresses, believing one the guest made up would turn a
        // guest bug into a host memory access.
        assert!(!super::is_issued(0x1234_5678));
        assert!(!super::is_issued(NO_THREAD));
    }

    #[test]
    fn a_host_thread_the_guest_did_not_make_reports_no_thread() {
        // The process's first thread is one of these, and the guest asks about it. A
        // fabricated handle there would be a handle nothing can join.
        assert_eq!(super::current(), NO_THREAD);
    }

    #[test]
    fn claiming_a_thread_is_visible_only_on_that_thread() {
        // `scePthreadSelf` must answer per-thread or every thread believes it is the
        // same one, and any per-thread bookkeeping the guest keeps collapses.
        super::become_thread(42);
        assert_eq!(super::current(), 42);

        let other = std::thread::spawn(super::current).join().expect("joined");
        assert_eq!(other, NO_THREAD, "a fresh thread inherits nothing");
        super::become_thread(NO_THREAD);
    }

    #[test]
    fn thread_stacks_are_spaced_further_apart_than_they_are_tall() {
        // Packed adjacently, an overrun on one stack lands in the next and reads as
        // memory corruption instead of as a stack overflow.
        let gap = super::stack_base_for(1) - super::stack_base_for(0);
        assert!(
            gap > orbistoun_mem::stack::DEFAULT_STACK_SIZE,
            "a stack must not be able to reach its neighbour"
        );
    }

    #[test]
    fn the_default_policy_records_rather_than_places() {
        // Deliberately not chosen because it is easiest: no title has been shown to
        // depend on placement, and a mapping invented before evidence exists is a guess
        // that later reads as a measurement.
        assert_eq!(AffinityPolicy::default(), AffinityPolicy::Observe);
    }
}
