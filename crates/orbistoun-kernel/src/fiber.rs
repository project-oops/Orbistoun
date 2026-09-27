//! User-mode fibers (`libSceFiber`): guest code on a stack the guest supplies, run and suspended
//! by explicit calls on whichever thread asks.
//!
//! A fiber is an execution context, not a thread (D732). `sceFiberRun` suspends the calling
//! thread's context inside the call and resumes the fiber on the same host thread; the fiber
//! suspends itself with `sceFiberReturnToThread`, which resumes the thread inside its
//! `sceFiberRun`, or hands the thread to another fiber with `sceFiberSwitch`. The context memory
//! is the fiber's stack: a first run enters `entry(argOnInitialize, argOnRun)` at its top, and
//! every later resume returns from the call that suspended it. [`orbistoun_abi::context`] does the
//! switching; this module keeps the books.
//!
//! The books are here, not in the guest's `SceFiber` object, whose layout nothing has measured:
//! a record per fiber address, a per-thread note of the fiber running on it, and a per-thread
//! slot where the thread side is suspended. A fiber's resume pointer is its state - non-zero when
//! suspended, taken (zeroed) by whoever resumes it - so two threads can never run one fiber, and
//! nothing is locked across a switch.
//!
//! A fiber suspended on one host thread may be resumed on another, so the handlers here read
//! thread-local state only through helpers that are never inlined: an address computed before a
//! switch could name the thread it began on.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use orbistoun_abi::context;
use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceFiber" {
        // (fiber, name, entry, argOnInitialize, addrContext, sizeContext) in registers; an option
        // block and a build version follow on the stack and are not used.
        "_sceFiberInitializeImpl" => 6,
        // (fiber, argOnRunTo, *argOnReturn).
        "sceFiberRun" => 3,
        // (fiber, argOnRunTo, *argOnRun).
        "sceFiberSwitch" => 3,
        // (argOnReturn, *argOnRun).
        "sceFiberReturnToThread" => 2,
        "sceFiberFinalize" => 1,
    }
}

/// `_sceFiberInitializeImpl` with no fiber object: obSCEne `033-fiber/invalid-args`, `null-fiber`.
pub const NULL_FIBER: u32 = 0x8059_0001;

/// `_sceFiberInitializeImpl` with a null context address or a zero context size: obSCEne
/// `033-fiber/invalid-args`, `null-stack` and `zero-stack`, both this code.
pub const BAD_CONTEXT: u32 = 0x8059_0004;

/// Success, as `sceFiberRun`, `_sceFiberInitializeImpl` and `sceFiberFinalize` answered it in
/// obSCEne `033-fiber/lifecycle`.
const OK: u64 = 0;

/// The longest fiber name kept, for logs; a longer one is cut here, not refused.
const NAME_KEPT: usize = 64;

/// One fiber the guest initialised.
#[derive(Debug)]
struct Fiber {
    /// What the guest called it, for logs.
    name: String,
    /// Its context memory, `(address, size)`, for logs.
    context: (u64, u64),
    /// Where it resumes: the stack pointer of its suspended context, or zero while it runs.
    ///
    /// Written by the switch that suspends it, as the last act on its stack, and taken by
    /// whatever resumes it. A zero here is the only "running" there is.
    resume: AtomicU64,
}

/// Every initialised fiber, by the address of its guest object.
fn fibers() -> &'static Mutex<BTreeMap<u64, Arc<Fiber>>> {
    static FIBERS: OnceLock<Mutex<BTreeMap<u64, Arc<Fiber>>>> = OnceLock::new();
    FIBERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// The record for the fiber object at `address`.
fn fiber_at(address: u64) -> Option<Arc<Fiber>> {
    fibers().lock().ok()?.get(&address).cloned()
}

thread_local! {
    /// The fiber this host thread is running, or zero while it runs its own context.
    static CURRENT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Where this thread's own context is suspended while it runs a fiber, or zero.
    static THREAD_SIDE: AtomicU64 = const { AtomicU64::new(0) };
}

/// The fiber running on this thread, or zero. Never inlined; see the module note.
#[inline(never)]
fn current() -> u64 {
    CURRENT.with(std::cell::Cell::get)
}

/// Records which fiber this thread is about to run. Never inlined; see the module note.
#[inline(never)]
fn set_current(fiber: u64) {
    CURRENT.with(|held| held.set(fiber));
}

/// Where a switch saves this thread's own context. Never inlined; see the module note.
#[inline(never)]
fn thread_side() -> *mut u64 {
    THREAD_SIDE.with(AtomicU64::as_ptr)
}

/// Takes the suspended thread context of this thread, leaving none. Never inlined; see the
/// module note.
#[inline(never)]
fn take_thread_side() -> u64 {
    THREAD_SIDE.with(|held| held.swap(0, Ordering::AcqRel))
}

/// A refusal of an argument whose platform code is unmeasured.
fn refused() -> u64 {
    u64::from(GuestError::InvalidArgument.as_raw())
}

/// A refusal naming an object nothing initialised, whose platform code is unmeasured.
fn unknown() -> u64 {
    u64::from(GuestError::InvalidHandle.as_raw())
}

/// Hands `value` to the guest at `out`, when it asked for it.
fn deliver(out: u64, value: u64) {
    if out != 0 {
        // SAFETY: an out-parameter the guest passed for this call, valid by its contract.
        unsafe { guest::write_u64(out, value) };
    }
}

/// Reached when a fiber's entry function returns, which ends the fiber with nowhere to go.
///
/// The platform's answer is unmeasured, and there is no caller to refuse to: the entry's caller
/// is the start routine at the top of the fiber's stack. So the run stops, naming the fiber.
extern "sysv64" fn entry_returned(fiber: u64) -> ! {
    let name = fiber_at(fiber).map_or_else(String::new, |f| f.name.clone());
    tracing::error!(
        "fiber {fiber:#x} ({name:?}) returned from its entry function; what the platform does then is unmeasured"
    );
    orbistoun_core::stop(orbistoun_core::StopReason::Unmeasured, fiber)
}

/// The seventh and eighth arguments of the call in progress, from the guest's stack, or zeros when
/// the dispatch published no stack area (a direct call, as in a test).
fn stack_words() -> (u64, u64) {
    let spilled = orbistoun_thunk::stack_arguments();
    if spilled == 0 {
        return (0, 0);
    }
    // SAFETY: the dispatch published this as the words above the return address the guest's call
    // pushed, on the calling thread's live stack.
    let seventh = unsafe { guest::read_u64(spilled) }.unwrap_or_default();
    // SAFETY: as above, the next word up.
    let eighth = unsafe { guest::read_u64(spilled + 8) }.unwrap_or_default();
    (seventh, eighth)
}

/// `_sceFiberInitializeImpl(fiber, name, entry, argOnInitialize, addrContext, sizeContext, ...)`:
/// makes the context memory a fiber that starts at `entry`.
///
/// The measured refusals come first: no fiber object, then no context memory or none of it. A
/// missing entry or a context too small for its first frame is refused with a placeholder, since
/// the platform's code for either is unmeasured. The first frame is written at the top of the
/// context now, carrying this thread's float control state, so the first run is an ordinary
/// resume. Re-initialising a fiber that is not running replaces it; one that is running is
/// refused. The option block and build version on the stack are not read.
fn initialize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [fiber, name, entry, argument, address, size] = *args;
    if fiber == 0 {
        return u64::from(NULL_FIBER);
    }
    if address == 0 || size == 0 {
        return u64::from(BAD_CONTEXT);
    }
    if entry == 0 {
        return refused();
    }
    let Some(top) = address.checked_add(size) else {
        return refused();
    };
    let start = context::Start {
        entry,
        argument,
        on_return: entry_returned as *const () as usize as u64,
        identity: fiber,
        float_control: context::float_control(),
    };
    let Some((pointer, words)) = context::initial_frame(top, &start) else {
        return refused();
    };
    if pointer < address {
        return refused();
    }

    let Ok(mut table) = fibers().lock() else {
        return refused();
    };
    if table
        .get(&fiber)
        .is_some_and(|old| old.resume.load(Ordering::Acquire) == 0)
    {
        return refused();
    }
    for (index, word) in words.iter().enumerate() {
        // SAFETY: `[address, address + size)` is the context memory the guest handed over for
        // this fiber, and `pointer` plus the frame lies inside it, as checked above.
        if !unsafe { guest::write_u64(pointer + index as u64 * 8, *word) } {
            return refused();
        }
    }
    let name = if name == 0 {
        String::new()
    } else {
        // SAFETY: a string the guest passed for this call, valid by its contract.
        unsafe { guest::read_cstr(name, NAME_KEPT) }
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    // The two stack words, used by nothing here, logged so a run shows what a title passes.
    let (option, version) = stack_words();
    tracing::debug!(
        "fiber {fiber:#x} ({name:?}) at entry {entry:#x}, context {address:#x}+{size:#x}, option block {option:#x}, stack word {version:#x}"
    );
    table.insert(
        fiber,
        Arc::new(Fiber {
            name,
            context: (address, size),
            resume: AtomicU64::new(pointer),
        }),
    );
    OK
}

/// `sceFiberRun(fiber, argOnRunTo, *argOnReturn)`: runs a fiber on this thread until it returns
/// the thread.
///
/// The fiber receives `argOnRunTo` - as its entry's second argument on a first run, through its
/// own suspending call's out-parameter otherwise - and this call returns once some fiber on this
/// thread calls `sceFiberReturnToThread`, whose value lands in `*argOnReturn`. Refused, with
/// placeholders since the codes are unmeasured, for a fiber nobody initialised, for one that is
/// running, and from a thread already running a fiber.
fn run(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [fiber, value, out, ..] = *args;
    if fiber == 0 || current() != 0 {
        return refused();
    }
    let Some(record) = fiber_at(fiber) else {
        return unknown();
    };
    let target = record.resume.swap(0, Ordering::AcqRel);
    // Dropped before the switch, so a frame suspended here owns nothing.
    drop(record);
    if target == 0 {
        return refused();
    }
    set_current(fiber);
    // SAFETY: `thread_side` is this thread's own slot, live for the thread's life; `target` was
    // taken from a suspended fiber's record, so it is a context `switch` saved or `initialize`
    // laid out, and nothing else can resume it now it is taken. This handler's frames own nothing
    // a suspension could invalidate.
    let back = unsafe { context::switch(thread_side(), target, value) };
    // Resumed by `sceFiberReturnToThread`, which cleared the running fiber.
    deliver(out, back);
    OK
}

/// `sceFiberSwitch(fiber, argOnRunTo, *argOnRun)`: suspends the running fiber and runs another on
/// this thread.
///
/// The thread side stays where it is, so the next `sceFiberReturnToThread` on this thread, from
/// either fiber, resumes it. When something resumes this fiber, the value it passes lands in
/// `*argOnRun`. Refused, with placeholders, off a fiber, for a fiber nobody initialised, and for
/// one that is running - this one included.
fn switch(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [fiber, value, out, ..] = *args;
    let running = current();
    if running == 0 || fiber == 0 {
        return refused();
    }
    let Some(own) = fiber_at(running) else {
        return unknown();
    };
    let Some(record) = fiber_at(fiber) else {
        return unknown();
    };
    let target = record.resume.swap(0, Ordering::AcqRel);
    drop(record);
    if target == 0 {
        return refused();
    }
    let save = own.resume.as_ptr();
    // The table keeps the record alive until this fiber is finalised, and that is refused while
    // its resume pointer reads zero, which it does until the switch below writes it.
    drop(own);
    set_current(fiber);
    // SAFETY: `save` points into this fiber's record, kept alive as the comment above says, and
    // `target` was taken from a suspended fiber, so only this switch resumes it.
    let back = unsafe { context::switch(save, target, value) };
    deliver(out, back);
    OK
}

/// `sceFiberReturnToThread(argOnReturn, *argOnRun)`: suspends the running fiber and resumes the
/// thread inside its `sceFiberRun`, which returns `argOnReturn` through its out-parameter.
///
/// When something later resumes this fiber, the value it passes lands in `*argOnRun`. Refused,
/// with a placeholder, off a fiber.
fn return_to_thread(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [value, out, ..] = *args;
    let running = current();
    if running == 0 {
        return refused();
    }
    let Some(own) = fiber_at(running) else {
        return unknown();
    };
    let target = take_thread_side();
    if target == 0 {
        return refused();
    }
    let save = own.resume.as_ptr();
    // As in `switch`: the table holds the record until the write below makes it finalisable.
    drop(own);
    set_current(0);
    // SAFETY: `save` points into this fiber's record, alive as above; `target` is where this
    // thread's own context suspended itself in `sceFiberRun`, taken so nothing else resumes it.
    let back = unsafe { context::switch(save, target, value) };
    deliver(out, back);
    OK
}

/// `sceFiberFinalize(fiber)`: forgets a fiber that is not running.
///
/// The context memory is the guest's and is left as it is. A suspended fiber may be finalised,
/// as obSCEne's lifecycle check does to one suspended inside its entry; its frames are never
/// resumed. Refused, with placeholders, for a fiber nobody initialised and for one that is running.
fn finalize(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let fiber = args[0];
    let Ok(mut table) = fibers().lock() else {
        return refused();
    };
    let Some(record) = table.get(&fiber) else {
        return unknown();
    };
    if record.resume.load(Ordering::Acquire) == 0 {
        return refused();
    }
    let (address, size) = record.context;
    tracing::debug!(
        "fiber {fiber:#x} ({:?}) finalised, context {address:#x}+{size:#x}",
        record.name
    );
    table.remove(&fiber);
    OK
}

/// Implementations this module provides, by symbol name.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("_sceFiberInitializeImpl", initialize),
        ("sceFiberRun", run),
        ("sceFiberSwitch", switch),
        ("sceFiberReturnToThread", return_to_thread),
        ("sceFiberFinalize", finalize),
    ]
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::{BAD_CONTEXT, NULL_FIBER, OK, finalize, initialize, return_to_thread, run, switch};
    use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Context memory for a test fiber, and the fiber object's own storage beside it.
    ///
    /// The object is never read by this implementation, but its address is the fiber's identity,
    /// so each test owns distinct storage.
    struct Fixture {
        object: Box<[u64; 32]>,
        context: Vec<u64>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                object: Box::new([0; 32]),
                context: vec![0; 8 * 1024],
            }
        }

        fn fiber(&self) -> u64 {
            self.object.as_ptr() as u64
        }

        fn context(&self) -> (u64, u64) {
            (
                self.context.as_ptr() as u64,
                (self.context.len() * 8) as u64,
            )
        }

        /// Initialises the fiber with `entry` and `argument`, asserting success.
        fn initialize(&self, entry: extern "sysv64" fn(u64, u64), argument: u64) {
            let (address, size) = self.context();
            let args = [
                self.fiber(),
                c"test-fiber".as_ptr() as u64,
                entry as *const () as usize as u64,
                argument,
                address,
                size,
            ];
            assert_eq!(initialize(&args), OK, "initialise");
        }
    }

    fn call(f: fn(&[u64; GUEST_ARG_REGISTERS]) -> u64, head: &[u64]) -> u64 {
        let mut args = [0; GUEST_ARG_REGISTERS];
        args[..head.len()].copy_from_slice(head);
        f(&args)
    }

    const PLACEHOLDER_ARGUMENT: u64 = GuestError::InvalidArgument.as_raw() as u64;

    /// The three measured refusals, with the argument order obSCEne's probe used.
    #[test]
    fn initialize_refuses_as_the_hardware_did() {
        let fixture = Fixture::new();
        let entry = lifecycle_entry as *const () as usize as u64;
        let (address, _) = fixture.context();
        let name = c"bad".as_ptr() as u64;
        assert_eq!(
            call(initialize, &[0, name, entry, 0, address, 4096]),
            u64::from(NULL_FIBER),
            "033-fiber/invalid-args null-fiber"
        );
        assert_eq!(
            call(initialize, &[fixture.fiber(), name, entry, 0, address, 0]),
            u64::from(BAD_CONTEXT),
            "033-fiber/invalid-args zero-stack"
        );
        assert_eq!(
            call(initialize, &[fixture.fiber(), name, entry, 0, 0, 4096]),
            u64::from(BAD_CONTEXT),
            "033-fiber/invalid-args null-stack"
        );
    }

    static LIFECYCLE: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

    extern "sysv64" fn lifecycle_entry(on_initialize: u64, on_run: u64) {
        LIFECYCLE[0].store(1, Ordering::SeqCst);
        LIFECYCLE[1].store(on_initialize, Ordering::SeqCst);
        LIFECYCLE[2].store(on_run, Ordering::SeqCst);
        call(return_to_thread, &[0x7788, 0]);
    }

    /// obSCEne's lifecycle check, reproduced: the entry receives both arguments, returns the
    /// thread, and the fiber, still suspended inside its entry, is finalised.
    #[test]
    fn the_measured_lifecycle_runs_the_entry_with_both_arguments() {
        let fixture = Fixture::new();
        fixture.initialize(lifecycle_entry, 0x1122);
        let mut returned = 0_u64;
        assert_eq!(
            call(run, &[fixture.fiber(), 0x3344, &raw mut returned as u64]),
            OK,
            "sceFiberRun rc"
        );
        assert_eq!(LIFECYCLE[0].load(Ordering::SeqCst), 1, "fiber-ran");
        assert_eq!(
            LIFECYCLE[1].load(Ordering::SeqCst),
            0x1122,
            "arg-init-received"
        );
        assert_eq!(
            LIFECYCLE[2].load(Ordering::SeqCst),
            0x3344,
            "arg-run-received"
        );
        assert_eq!(
            returned, 0x7788,
            "the value the fiber returned the thread with"
        );
        assert_eq!(
            call(finalize, &[fixture.fiber()]),
            OK,
            "sceFiberFinalize rc"
        );
    }

    static RESUME: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

    extern "sysv64" fn resuming_entry(on_initialize: u64, on_run: u64) {
        let local = std::hint::black_box([on_initialize, on_run, 0xF1BE_F1BE]);
        let mut next = 0_u64;
        call(return_to_thread, &[1, &raw mut next as u64]);
        let seen = std::hint::black_box(local);
        RESUME[0].store(
            u64::from(seen == [on_initialize, on_run, 0xF1BE_F1BE]),
            Ordering::SeqCst,
        );
        RESUME[1].store(next, Ordering::SeqCst);
        call(return_to_thread, &[2, &raw mut next as u64]);
        RESUME[2].store(next, Ordering::SeqCst);
        call(return_to_thread, &[3, 0]);
    }

    /// Run, return to the thread, run again: the fiber resumes where it left off, with its stack
    /// intact and each run's value delivered through its suspending call.
    #[test]
    fn a_fiber_run_again_resumes_where_it_returned_the_thread() {
        let fixture = Fixture::new();
        fixture.initialize(resuming_entry, 0xA);
        let mut back = 0_u64;
        let out = &raw mut back as u64;

        assert_eq!(call(run, &[fixture.fiber(), 0xB, out]), OK);
        assert_eq!(back, 1, "the first return");
        assert_eq!(call(run, &[fixture.fiber(), 0xC, out]), OK);
        assert_eq!(back, 2, "resumed, not restarted");
        assert_eq!(RESUME[0].load(Ordering::SeqCst), 1, "its stack survived");
        assert_eq!(
            RESUME[1].load(Ordering::SeqCst),
            0xC,
            "the second run's value"
        );
        assert_eq!(call(run, &[fixture.fiber(), 0xD, out]), OK);
        assert_eq!(back, 3);
        assert_eq!(
            RESUME[2].load(Ordering::SeqCst),
            0xD,
            "the third run's value"
        );
        assert_eq!(call(finalize, &[fixture.fiber()]), OK);
    }

    static PAIR_OBJECTS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
    static PAIR: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

    extern "sysv64" fn first_of_pair(_on_initialize: u64, on_run: u64) {
        PAIR[0].store(on_run, Ordering::SeqCst);
        let mut resumed_with = 0_u64;
        let second = PAIR_OBJECTS[1].load(Ordering::SeqCst);
        call(switch, &[second, 0x5EC0, &raw mut resumed_with as u64]);
        PAIR[2].store(resumed_with, Ordering::SeqCst);
        call(return_to_thread, &[0xF00D, 0]);
    }

    extern "sysv64" fn second_of_pair(_on_initialize: u64, on_run: u64) {
        PAIR[1].store(on_run, Ordering::SeqCst);
        // Refusals only a running fiber can provoke: running a fiber from a fiber, switching to
        // itself, and finalising itself.
        let own = PAIR_OBJECTS[1].load(Ordering::SeqCst);
        let refusals = u64::from(call(run, &[own, 0, 0]) == PLACEHOLDER_ARGUMENT)
            + u64::from(call(switch, &[own, 0, 0]) == PLACEHOLDER_ARGUMENT)
            + u64::from(call(finalize, &[own]) == PLACEHOLDER_ARGUMENT);
        PAIR[3].store(refusals, Ordering::SeqCst);
        call(return_to_thread, &[0xBEEF, 0]);
    }

    /// One fiber switches straight to another; the second returns the thread, and a later run of
    /// the first resumes it inside its switch with the run's value.
    #[test]
    fn a_fiber_switches_to_another_and_is_resumed_inside_its_switch() {
        let first = Fixture::new();
        let second = Fixture::new();
        first.initialize(first_of_pair, 0);
        second.initialize(second_of_pair, 0);
        PAIR_OBJECTS[0].store(first.fiber(), Ordering::SeqCst);
        PAIR_OBJECTS[1].store(second.fiber(), Ordering::SeqCst);
        let mut back = 0_u64;
        let out = &raw mut back as u64;

        assert_eq!(call(run, &[first.fiber(), 0x0A, out]), OK);
        assert_eq!(PAIR[0].load(Ordering::SeqCst), 0x0A, "the first ran");
        assert_eq!(
            PAIR[1].load(Ordering::SeqCst),
            0x5EC0,
            "and handed the second its value"
        );
        assert_eq!(back, 0xBEEF, "which returned the thread");
        assert_eq!(
            PAIR[3].load(Ordering::SeqCst),
            3,
            "after its three refusals"
        );

        assert_eq!(call(run, &[first.fiber(), 0x0B, out]), OK);
        assert_eq!(
            PAIR[2].load(Ordering::SeqCst),
            0x0B,
            "the first resumed inside its switch"
        );
        assert_eq!(back, 0xF00D);
        assert_eq!(call(finalize, &[first.fiber()]), OK);
        assert_eq!(call(finalize, &[second.fiber()]), OK);
    }

    /// Off a fiber there is nothing to return from or switch away from, and a fiber nobody
    /// initialised cannot be run or finalised.
    #[test]
    fn calls_that_need_a_fiber_refuse_without_one() {
        assert_eq!(call(return_to_thread, &[0, 0]), PLACEHOLDER_ARGUMENT);
        let fixture = Fixture::new();
        assert_eq!(call(switch, &[fixture.fiber(), 0, 0]), PLACEHOLDER_ARGUMENT);
        let never = u64::from(GuestError::InvalidHandle.as_raw());
        assert_eq!(call(run, &[fixture.fiber(), 0, 0]), never);
        assert_eq!(call(finalize, &[fixture.fiber()]), never);
    }

    /// A fiber can be run by a thread other than the one that initialised it or last ran it.
    #[test]
    fn a_suspended_fiber_resumes_on_another_host_thread() {
        let fixture = Fixture::new();
        fixture.initialize(migrating_entry, 0);
        let fiber = fixture.fiber();
        let mut back = 0_u64;
        assert_eq!(call(run, &[fiber, 1, &raw mut back as u64]), OK);
        assert_eq!(back, 10);
        let elsewhere = std::thread::spawn(move || {
            let mut back = 0_u64;
            let rc = call(run, &[fiber, 2, &raw mut back as u64]);
            (rc, back)
        })
        .join()
        .expect("the other thread finishes");
        assert_eq!(elsewhere, (OK, 20), "resumed on the second thread");
        assert_eq!(call(finalize, &[fiber]), OK);
    }

    extern "sysv64" fn migrating_entry(_on_initialize: u64, on_run: u64) {
        let mut next = 0_u64;
        call(return_to_thread, &[on_run * 10, &raw mut next as u64]);
        call(return_to_thread, &[next * 10, 0]);
    }
}
