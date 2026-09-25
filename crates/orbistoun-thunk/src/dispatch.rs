//! Where every thunk lands, and what it records.
//!
//! One naked trampoline serves the whole table. Each thunk arrives having put its own
//! index in `r10`, which is scratch under System V and therefore the one register that
//! can carry a value in without destroying an argument.
//!
//! # The trampoline exists because Rust cannot read a register
//!
//! An ordinary function has no way to see `r10`. Something has to spill the argument
//! registers to memory and re-present them as an ordinary call, and that something has
//! to be hand-written. It is the smallest piece of assembly that will do it, and it is
//! written once rather than once per import.
//!
//! # Stack alignment, which is easy to get wrong and silent when wrong
//!
//! System V requires `rsp % 16 == 0` immediately before a `call`, so a callee sees
//! `rsp % 16 == 8` on entry. The guest satisfied that when it called the thunk, so this
//! arrives at `rsp % 16 == 8`. The `sub rsp, 8` restores alignment before the six
//! pushes - which move a multiple of 16 and preserve it - so the handler is entered
//! correctly. Getting this wrong does nothing at all until some callee executes an
//! aligned SSE instruction against a stack slot, and then faults far from the cause.
//!
//! # Recording obeys the rule that observing must not change the program
//!
//! No allocation and no locks on this path (principle 9). Counters are allocated once
//! when the table is built; the call path only ever does a relaxed atomic add. A
//! bounded ring keeps the first calls **in order**, which is what makes a boot trace
//! readable - a plain histogram loses the sequence, and the sequence is the part that
//! says what the guest was trying to do.

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use orbistoun_core::GuestError;

/// How many calls are kept in order before only counts are recorded.
///
/// Bounded on purpose: an unbounded log would allocate on the call path, and the
/// interesting part of a boot is its beginning - by the ten-thousandth call the guest
/// is in a loop, and the counters already say so.
pub const MAX_RECORDED_CALLS: usize = 8192;

/// Number of argument registers System V passes in.
///
/// The same count the subsystem crates write implementations against, so the trampoline
/// and the functions it calls cannot disagree about how many it spilled.
pub const SAVED_ARGUMENT_REGISTERS: usize = orbistoun_core::GUEST_ARG_REGISTERS;

/// What `rsp % 16` must be when a callee is entered.
///
/// System V requires `rsp % 16 == 0` immediately *before* a `call`. The call pushes an
/// eight-byte return address, so the callee begins life eight past alignment. Every
/// compiler relies on this: it does its own arithmetic from that starting point to line
/// the stack up before using an instruction that moves sixteen bytes at once.
pub const EXPECTED_ENTRY_REMAINDER: u64 = 8;

/// Whether a stack pointer at a callee's first instruction obeys the convention.
///
/// Pure, so the rule can be tested without a guest. That matters here more than usual:
/// the code that uses it runs inside a naked trampoline, where a mistake is invisible
/// until it is catastrophic.
pub const fn entry_alignment_conforms(entry_rsp: u64) -> bool {
    entry_rsp % 16 == EXPECTED_ENTRY_REMAINDER
}

/// Calls that arrived on a stack the convention says is impossible.
static MISALIGNED_CALLS: AtomicU64 = AtomicU64::new(0);
/// Sequence number of the first such call, or `u64::MAX` if there has not been one.
static FIRST_MISALIGNED_SEQUENCE: AtomicU64 = AtomicU64::new(u64::MAX);
/// The `rsp` that first broke the rule, kept whole rather than reduced - the full value
/// says which region the stack was in, which the remainder alone cannot.
static FIRST_MISALIGNED_RSP: AtomicU64 = AtomicU64::new(0);
/// Import index of the first offender.
static FIRST_MISALIGNED_INDEX: AtomicU64 = AtomicU64::new(0);

/// What the guest's calls looked like against the calling convention.
///
/// **Telemetry rather than a check.** Nothing here refuses a call or corrects a stack:
/// forcing alignment would make the symptom vanish while leaving the guest running
/// misaligned internally, which converts a loud immediate fault into silent corruption
/// somewhere unattributable (D159).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AbiConformance {
    /// How many calls arrived on a stack the convention forbids.
    pub misaligned_calls: u64,
    /// The first one, if there was one: `(sequence, import index, rsp)`.
    pub first_misaligned: Option<(u64, u32, u64)>,
}

/// How the guest's calls measured against the convention.
pub fn abi_conformance() -> AbiConformance {
    let first = FIRST_MISALIGNED_SEQUENCE.load(Ordering::Relaxed);
    AbiConformance {
        misaligned_calls: MISALIGNED_CALLS.load(Ordering::Relaxed),
        first_misaligned: (first != u64::MAX).then(|| {
            (
                first,
                FIRST_MISALIGNED_INDEX.load(Ordering::Relaxed) as u32,
                FIRST_MISALIGNED_RSP.load(Ordering::Relaxed),
            )
        }),
    }
}

/// Where this thread's current call keeps the arguments that did not fit in registers.
///
/// # Why a variadic call needs it
///
/// System V passes the first six integer arguments in registers and **the rest on the
/// stack**, immediately above the return address. The trampoline spills the six and hands
/// them over as an array, and everything with a fixed signature is happy: nothing here takes
/// more than six.
///
/// `printf` does. `zftpd` answers a passive-mode request with
/// `"227 Entering Passive Mode (%d,%d,%d,%d,%d,%d)"` - a format, a buffer, a size and six
/// numbers, which is nine arguments and three registers left for the six. Its client saw
/// `227` and nothing after it, and the same truncation emptied every `[FTP][INFO]` line and
/// the path out of every `257` reply (D385).
///
/// So the overflow area is published for the length of the call, and the renderer reads it
/// when the registers run out. That is not a trick: it is the `overflow_arg_area` of the
/// `va_list` the psABI defines, reached from the other side.
///
/// # What it does not fix
///
/// The count is still the format string's word. Reading past what the guest actually passed
/// gives whatever the stack held - exactly the risk a real `printf` has, for exactly the same
/// reason, and the reason a wrong format is a bug in any C program.
///
/// Zero when no guest call is in progress, or when the stack pointer was not one.
mod overflow {
    use std::cell::Cell;

    thread_local! {
        /// The first stack argument of the call this thread is inside.
        static AREA: Cell<u64> = const { Cell::new(0) };
    }

    /// Publishes the area for a call, answering what was there before.
    ///
    /// **Saved and restored rather than cleared**, because an implementation that calls back
    /// into another import would otherwise leave the outer call reading nothing.
    pub(super) fn begin(entry_rsp: u64) -> u64 {
        // `[entry_rsp]` is the return address the guest's `call` pushed, so the first
        // argument that did not fit is the word above it.
        let area = if entry_rsp == 0 || entry_rsp % 8 != 0 {
            0
        } else {
            entry_rsp.saturating_add(8)
        };
        AREA.with(|held| held.replace(area))
    }

    /// Puts back what `begin` answered.
    pub(super) fn end(previous: u64) {
        AREA.with(|held| held.set(previous));
    }

    /// Where this thread's current call keeps its stack arguments, or zero.
    pub fn area() -> u64 {
        AREA.with(Cell::get)
    }
}

pub use overflow::area as stack_arguments;

/// Where a call came from, given the stack pointer as the guest's `call` left it.
///
/// A `call` pushes the return address and leaves `rsp` pointing at it, and the thunk
/// reaches the trampoline by a `jmp`, which pushes nothing - so that word is still the top
/// of the stack when this runs.
///
/// Zero when the pointer is not word-aligned, which would mean the convention was violated
/// badly enough that the word there is not a return address. Reading it anyway would put a
/// fabricated address into a trace people navigate by.
fn call_site(entry_rsp: u64) -> u64 {
    if entry_rsp == 0 || entry_rsp % 8 != 0 {
        return 0;
    }
    let Ok(at) = usize::try_from(entry_rsp) else {
        return 0;
    };
    // SAFETY: the processor pushed a return address at exactly this location a moment ago
    // to reach the thunk that jumped here, so the word is present and readable. Reading it
    // does not disturb the guest's stack.
    unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at)) }
}

/// Records one call's incoming stack alignment.
///
/// Two relaxed adds in the common case and nothing else - observing must not change the
/// program it observes (principle 9). The "first" fields are written with a compare-and-
/// swap on the sequence so the earliest offender wins even when threads race, and the
/// earliest is the one that matters: later misalignment is usually the first one's
/// consequence.
fn record_alignment(sequence: u64, index: u64, entry_rsp: u64) {
    if entry_alignment_conforms(entry_rsp) {
        return;
    }
    MISALIGNED_CALLS.fetch_add(1, Ordering::Relaxed);
    if FIRST_MISALIGNED_SEQUENCE
        .compare_exchange(u64::MAX, sequence, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        FIRST_MISALIGNED_INDEX.store(index, Ordering::Relaxed);
        FIRST_MISALIGNED_RSP.store(entry_rsp, Ordering::Relaxed);
    }
}

/// Total calls seen, and the source of each record's ordering.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The most recent calls. Stores `index + 1` so zero means "nothing here".
///
/// **Circular, since D571.** It used to fill once and stop, which made `CallTrace::tail` -
/// documented as *the last calls the guest made*, printed as `last calls before the fault`, and
/// given its purpose by D154 as the neighbourhood of the wall - be calls #8,144-#8,191 of runs
/// making four hundred thousand and twelve million (D568). It now holds the last
/// [`MAX_RECORDED_CALLS`], which is what every reader of it wanted.
static RING: [AtomicU64; MAX_RECORDED_CALLS] = [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// Which call each slot holds, as `sequence + 1`, parallel to [`RING`].
///
/// **Needed only because the ring wraps.** While it filled once a slot's position *was* its
/// sequence and nothing had to be stored; wrapped, slot 3 might hold call 3 or call 8,195, and a
/// reader with no way to tell would report them in the wrong order.
static RING_SEQ: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// How many of a run's *first* calls are kept whatever else happens.
///
/// The circular ring answers "what did it call last"; this answers "what did it call first", and
/// they are different questions with different readers. The halt summary quotes eight (D571).
///
/// **Two thousand and forty-eight rather than eight**, because a second reader arrived with a
/// different question: *what did the guest do differently between call 219 and call 226?* One
/// mapping appears in some runs of PPSA03416 and not others, and that single branch accounts for
/// the whole 192-against-193 drift (D602) - so the calls around it are the evidence, and eight
/// could not reach them.
///
/// The cost is what it always was: a store per call while the count is below this, and a load
/// afterwards. Two thousand stores in a run of four hundred and sixty-seven thousand calls is
/// not a sink that changes what it observes; keeping *every* call would be (D603).
pub const OPENING_CALLS: usize = 2048;

/// The opening of the run: `index + 1` for the first [`OPENING_CALLS`] calls, never overwritten.
static RING_OPENING: [AtomicU64; OPENING_CALLS] = [const { AtomicU64::new(0) }; OPENING_CALLS];

/// Where each opening call came from, parallel to [`RING_OPENING`].
///
/// **A name says what ran; an address says which code ran it.** The same argument D596 makes for
/// a formatted message: a title calls `memcpy` from four hundred places, and knowing which one is
/// the difference between a list and a lead.
static RING_OPENING_FROM: [AtomicU64; OPENING_CALLS] = [const { AtomicU64::new(0) }; OPENING_CALLS];

/// The opening record must be the cheap one, or keeping it separately buys nothing.
///
/// **A compile-time assertion, not a test.** Written first as one, it was a comparison of two
/// constants that no runtime could make fail - coverage in appearance and nothing underneath.
/// Clippy said so, and the right home for an invariant a compiler can settle is the compiler
/// (D571).
const _: () = assert!(OPENING_CALLS < MAX_RECORDED_CALLS);

/// Every integer argument of each recorded call, parallel to [`RING`], six per call.
///
/// # Why all six and not just the first
///
/// It was `arg0` alone, and that made a whole class of evidence invisible: a placeholder handed to
/// a function as a **size** shows up only if the size happens to be the first argument. The four
/// gigabytes of D564 were visible purely because `malloc` takes its size there; the same value
/// passed as `arg2` would have left no trace at all, and an attempt to classify unimplemented
/// functions by what the guest does with their answers found exactly one usable observation across
/// four runs because of it (D570).
///
/// **It costs nothing on the hot path**, which is the reason this was cheap: recording already
/// stops after [`MAX_RECORDED_CALLS`], so the extra stores are bounded at that many times five
/// however long the guest runs. The static cost is the arrays themselves.
static RING_ARGS: [AtomicU64; MAX_RECORDED_CALLS * SAVED_ARGUMENT_REGISTERS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS * SAVED_ARGUMENT_REGISTERS];

/// Where each recorded call came *from*, parallel to [`RING`].
///
/// **Principle 9's actual requirement**, unmet until now: *"which function" is the wrong
/// question - "which call site" is the right one.* A count says a title calls `memset`
/// three hundred times; a call site says which three places, and that is what turns a
/// trace into a map of the guest's own code (D173).
///
/// Free to capture. The trampoline already carries the stack pointer as the guest's `call`
/// left it, and the return address is the word sitting at exactly that address.
/// Which host thread made each call, as its own identifier.
///
/// **So a reader can tell which of the recent calls happened on the thread that faulted.** The
/// tail is every thread's, and a fault is one thread's; pairing them by eye is guesswork, and it
/// produced four decisions treating a wait blocked on one thread as the cause of a fault on
/// another (D621).
///
/// The host thread rather than the guest handle, because this crate is below the one that issues
/// guest handles and must not reach up for it. The report joins the two.
static RING_THREAD: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

static RING_FROM: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// What each recorded call **answered**, parallel to [`RING`].
///
/// **The other half of a call, and the half nothing recorded.** Every diagnostic on this
/// path captured what the guest *passed in* - the first argument, the pointers it dumped -
/// and none captured what our own function *handed back*. So the failure mode this project
/// hits most, an implemented function answering a wrong value the guest then trusts (the
/// D125 class), was invisible in the one record a person reads at a wall: the trace tail
/// showed the call happening and not what it returned (D459).
///
/// Written *after* the handler returns, so it needs its own "was it written" signal -
/// [`RING_RETURNED`] - because any `u64` is a legitimate answer and zero is the commonest
/// one (`OK`). A slot whose call is still running, or whose guest faulted in its own code
/// the instant the call returned, has no answer yet and must read as *unknown* rather than
/// as zero.
static RING_RET: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// Whether [`RING_RET`] holds a real answer for a slot yet: `1` once written, `0` before.
///
/// Stored with `Release` after the answer, and read with `Acquire`, so a reader that sees
/// the flag set is guaranteed to see the answer that goes with it and never a stale word.
static RING_RETURNED: [AtomicU8; MAX_RECORDED_CALLS] =
    [const { AtomicU8::new(0) }; MAX_RECORDED_CALLS];

/// Per-import call counts. Allocated once when a table is built, never on the call path.
static COUNTS: std::sync::OnceLock<Box<[AtomicU64]>> = std::sync::OnceLock::new();

/// The shape bits an argument value ORs into its slot's profile, so the kinds a slot saw across
/// calls accumulate into a picture of what that argument *is*.
///
/// The guest cannot describe its own imports - the firmware they resolve to is opaque and unreadable
/// (XOM) - but the guest's **own calls** describe them: a slot that is always a pointer into a guest
/// region is a pointer, one that is always small is a scalar or a size, one that is never anything
/// but zero is unused, and one that is sometimes a pointer and sometimes zero is an optional pointer.
/// This is inference from observed behaviour, the only signature source the black box allows, and it
/// is a lower bound (a real argument always passed zero reads as unused) - a characterisation, never
/// a proof.
pub const SHAPE_ZERO: u8 = 0x1;
/// A small value: a flag, a count, a size - not an address. See [`SHAPE_ZERO`].
pub const SHAPE_SCALAR: u8 = 0x2;
/// A value inside the guest's address space - a pointer. See [`SHAPE_ZERO`].
pub const SHAPE_POINTER: u8 = 0x4;
/// Anything else - a large non-address value. See [`SHAPE_ZERO`].
pub const SHAPE_OTHER: u8 = 0x8;

/// Classifies one argument value into its [shape bit](SHAPE_ZERO).
///
/// Pure, so the boundaries are tested without a running guest. The guest address space runs from the
/// image base up through the stack and the direct-memory pool (`0x4000…`..`0x8000…`); below a page a
/// value is a scalar, not an address.
#[must_use]
pub const fn classify_arg(v: u64) -> u8 {
    /// The bottom of the guest's address space (image base). Below the pool and stack, above every
    /// scalar a guest passes.
    const GUEST_LOW: u64 = 0x4000_0000_0000;
    /// One past the top of it.
    const GUEST_HIGH: u64 = 0x8000_0000_0000;
    /// Under a page: a flag, a small count, or a size - never an address.
    const SCALAR_CEILING: u64 = 0x1_0000;
    if v == 0 {
        SHAPE_ZERO
    } else if v < SCALAR_CEILING {
        SHAPE_SCALAR
    } else if v >= GUEST_LOW && v < GUEST_HIGH {
        SHAPE_POINTER
    } else {
        SHAPE_OTHER
    }
}

/// Renders one argument slot's accumulated [shape bits](SHAPE_ZERO) into a kind.
///
/// A register that has ever held a pointer is a pointer slot even if it was also null on other
/// calls, so the categories are read widest-first (pointer, then large non-address, then scalar); a
/// `?` marks a slot that was **also** zero on some call - a nullable pointer, an optional size. A
/// slot that was never anything but zero is an argument the guest always passed as nought: present
/// (the `describe_shape` arity counts it) but carrying no other evidence.
const fn describe_slot(bits: u8) -> &'static str {
    let nullable = bits & SHAPE_ZERO != 0;
    if bits & SHAPE_POINTER != 0 {
        if nullable { "ptr?" } else { "ptr" }
    } else if bits & SHAPE_OTHER != 0 {
        if nullable { "u64?" } else { "u64" }
    } else if bits & SHAPE_SCALAR != 0 {
        if nullable { "u32?" } else { "u32" }
    } else {
        "0"
    }
}

/// Renders an import's inferred argument shapes into a signature like `(ptr, u32, ptr?)`.
///
/// The arity is the count up to the last slot that ever carried anything: a register never written
/// across the sampled calls is one the guest did not pass, so the trailing run of untouched slots is
/// dropped and the parentheses report how many arguments the calls actually used. An import called
/// with no arguments renders `()`; all-zero (never sampled, or every argument always nought) does
/// too - the honest floor of a black-box read (see [`classify_arg`]).
#[must_use]
pub fn describe_shape(shape: &[u8]) -> String {
    let arity = shape
        .iter()
        .rposition(|&b| b != 0)
        .map_or(0, |last| last + 1);
    let mut out = String::from("(");
    for (register, &bits) in shape.iter().take(arity).enumerate() {
        if register != 0 {
            out.push_str(", ");
        }
        out.push_str(describe_slot(bits));
    }
    out.push(')');
    out
}

/// How many calls of each import contribute to its inferred argument shapes.
///
/// Bounded so the busiest imports - called tens of millions of times - pay the six extra stores only
/// at the start, where the shape is established in far fewer: a slot that is a pointer on sixteen
/// calls is a pointer, and the seventeenth adds nothing (principle 9).
const SHAPE_SAMPLE_LIMIT: u64 = 16;

/// Per-import argument shapes: [`SAVED_ARGUMENT_REGISTERS`] slots per import, each an OR of the
/// [shape bits](SHAPE_ZERO) its calls carried. Allocated with [`COUNTS`], never on the call path.
static SHAPES: std::sync::OnceLock<Box<[AtomicU8]>> = std::sync::OnceLock::new();

pub use orbistoun_core::GuestFn;

/// How many imports the guest may call before the run is stopped.
///
/// # Why a call budget exists next to a wall-clock limit
///
/// The wall-clock limit makes the *duration* fixed and the call count the varying
/// quantity, and the varying quantity is the one every verdict is read off. Three
/// identical runs of the same title returned 77.5M, 75.8M and 87.6M calls - a 13% spread
/// with no change to the build - so `FURTHER`/`same`/`BACK` was least trustworthy exactly
/// where a guest runs long enough to matter (D181, D194).
///
/// A budget inverts that: the count is fixed and the duration varies. Two runs of the same
/// build reach the same call and stop there, so a verdict between them measures the change
/// rather than the machine.
///
/// **It does not replace the wall-clock limit.** A guest that stops calling imports - an
/// idle loop waiting on something that never happens - never reaches a call budget at all,
/// and would hang. The two answer different failure modes and both are needed (D238).
///
/// Starts at `u64::MAX` so the ordinary path is one relaxed load and a comparison that is
/// always false, with no branch misprediction to pay for (principle 9).
static CALL_BUDGET: AtomicU64 = AtomicU64::new(u64::MAX);

/// What to do when the budget is reached. Installed by the worker, which owns the trace.
///
/// A callback rather than the stop itself: writing a trace means collecting, persisting and
/// summarising it, and all three live above this crate in the spine. Reaching up from here
/// would invert the dependency for no gain.
static ON_BUDGET: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Stops the run after `budget` import calls, by calling `on_exceeded`.
pub fn install_call_budget(budget: u64, on_exceeded: fn()) {
    let _ = ON_BUDGET.set(on_exceeded);
    CALL_BUDGET.store(budget, Ordering::Relaxed);
}

/// Implementations that speak in floating-point registers, by symbol index.
///
/// Separate from [`HANDLERS`] because almost nothing needs it: widening every implemented
/// function's signature to carry an unused array would put the cost on the ninety-nine
/// million calls that never touch one (D268).
static FLOAT_HANDLERS: std::sync::OnceLock<Box<[Option<orbistoun_core::GuestFloatFn>]>> =
    std::sync::OnceLock::new();

/// Records the floating-point implementations for a table of `count` entries.
pub fn install_float_handlers(handlers: Vec<Option<orbistoun_core::GuestFloatFn>>) {
    let _ = FLOAT_HANDLERS.set(handlers.into_boxed_slice());
}

/// Implementations by symbol index, or `None` where there is none yet.
///
/// **This is what makes implementing a function change anything.** Without it every
/// import lands on a stub that records and refuses, however much real code exists
/// elsewhere - the registry knew names and arities and nothing consulted it at the point
/// the guest actually calls (D082).
static HANDLERS: std::sync::OnceLock<Box<[Option<GuestFn>]>> = std::sync::OnceLock::new();

/// What an unimplemented stub hands back, by symbol index.
///
/// `None` means the ordinary error code. A pointer- or handle-returning function needs
/// zero instead, because the caller reads the answer as data rather than testing it -
/// and an error code sitting in a pointer register is a wild pointer the guest
/// dereferences immediately (D125).
static STUB_RETURNS: std::sync::OnceLock<Box<[Option<u64>]>> = std::sync::OnceLock::new();

/// Records what each unimplemented stub should answer.
pub fn install_stub_returns(values: Vec<Option<u64>>) {
    let _ = STUB_RETURNS.set(values.into_boxed_slice());
}

/// A forced answer per symbol index, overriding both the policy and the error code.
///
/// **Separate from [`STUB_RETURNS`] rather than folded into it**, because that one is
/// installed by the service from the policy file and is a `OnceLock` - a second install
/// is a silent no-op, which is exactly the failure mode this whole family of diagnostics
/// exists to avoid. A distinct layer, consulted first, cannot lose that race.
static FORCED_RETURNS: std::sync::OnceLock<Box<[Option<u64>]>> = std::sync::OnceLock::new();

/// How many calls were answered with a forced value.
static RETURNS_FORCED: AtomicU64 = AtomicU64::new(0);

/// Makes named imports answer a chosen value for one run.
///
/// # Why a diagnostic and not a policy entry
///
/// `StubPolicy` is keyed by symbol name and carries a 32-bit code. Both are right for
/// what it is - a human-editable file of established error codes - and neither can ask
/// the question at a wall: the function there has no name, so it cannot be keyed, and a
/// region base is 64-bit, so it could not be expressed.
///
/// The consequence was worse than an inconvenience. An override written for an unnamed
/// function matched nothing, fell back to the default, and produced a run that looked
/// like an experiment reporting no change - so "the return value is not where the base
/// comes back" was recorded as a measurement when nothing had been measured (D230).
pub fn install_forced_returns(values: Vec<Option<u64>>) {
    let _ = FORCED_RETURNS.set(values.into_boxed_slice());
}

/// The answer forced for `index`, counting it when there is one.
///
/// Consulted for implemented and unimplemented imports alike. The first version checked
/// only the unimplemented path, which meant the remaining explanation for the
/// `image+0xafc959` wall - an *implemented* function handing back zero-as-success where
/// the guest wants a pointer, the D125 class - was the one thing it could not test (D234).
fn forced_answer(index: u64) -> Option<u64> {
    let value = FORCED_RETURNS
        .get()?
        .get(usize::try_from(index).ok()?)
        .copied()
        .flatten()?;
    RETURNS_FORCED.fetch_add(1, Ordering::Relaxed);
    Some(value)
}

/// How many calls a forced return actually answered.
pub fn forced_return_count() -> u64 {
    RETURNS_FORCED.load(Ordering::Relaxed)
}

/// Installs the implementations for a table of `count` entries.
///
/// Indexed by dynamic symbol index so a lookup on the call path is one bounds-checked
/// read. Called once, at table construction, for the same reason the counters are.
pub fn install_handlers(handlers: Vec<Option<GuestFn>>) {
    let _ = HANDLERS.set(handlers.into_boxed_slice());
}

/// Whether a particular import has an implementation behind it.
///
/// Asked when a run is summarised rather than on the call path: "called and not
/// implemented" is the most directly actionable thing a run can report, and without this
/// the trace cannot tell that apart from "called and handled" (D179).
pub fn is_implemented(index: usize) -> bool {
    attached(&HANDLERS, index) || attached(&FLOAT_HANDLERS, index)
}

/// Whether one table has a handler in a slot.
///
/// **Both tables, always.** A function answering in `xmm0` is as implemented as one answering
/// in `rax`, and this asked only the integer one - so every maths function dispatched
/// correctly, passed its conformance check, and was recorded as a call nothing implemented.
/// That took argument dumps for functions whose integer registers hold leftovers, put finished
/// work at the top of the findings list, and understated `standing`, which is the number this
/// project reads as its own progress (D268, D290).
fn attached<T>(table: &std::sync::OnceLock<Box<[Option<T>]>>, index: usize) -> bool {
    table
        .get()
        .and_then(|t| t.get(index))
        .is_some_and(Option::is_some)
}

/// How many handlers are attached, across the whole table.
pub fn implemented_count() -> usize {
    implemented_count_within(usize::MAX)
}

/// How many of the first `limit` slots have an implementation behind them.
///
/// **The limit is what keeps a report honest.** The table carries stubs past the guest's
/// own imports, for names it may resolve at run time (D365), and every one of those has a
/// handler by construction. Counting them into "N imports, M implemented" would report a
/// module as far better served than it is - so a report that means the guest's imports
/// passes the guest's import count here.
pub fn implemented_count_within(limit: usize) -> usize {
    // The two tables are disjoint by construction - a function answers in one register or the
    // other, never both - so this is a sum rather than a union (D268).
    let counted = |table: &std::sync::OnceLock<Box<[Option<GuestFn>]>>| {
        table
            .get()
            .map_or(0, |h| h.iter().take(limit).filter(|f| f.is_some()).count())
    };
    let integers = counted(&HANDLERS);
    let floats = FLOAT_HANDLERS
        .get()
        .map_or(0, |h| h.iter().take(limit).filter(|f| f.is_some()).count());
    integers + floats
}

/// How many argument dumps a run keeps.
///
/// Small on purpose. A dump is only taken for an import nothing implements, and only for
/// its first few calls, so the interesting ones all arrive early - and a fixed ceiling is
/// what keeps this allocation-free on the call path (principle 9).
pub const MAX_DUMPS: usize = 512;

/// How many calls of one import are worth dumping.
///
/// The seventy-sixth `snprintf_s` tells you nothing the first did. Two, so that a value
/// which changes between calls can be told from one that does not - which is the
/// difference between an out-parameter and a constant.
const DUMPS_PER_IMPORT: u32 = 2;

/// Bytes captured from each argument that points somewhere readable.
///
/// Enough for a small struct or the start of a string. A guest that passes a bigger
/// structure still shows its first fields, and those are the ones that identify it - a
/// size, a version, a magic (D083).
pub const DUMP_BYTES: usize = 32;

/// Words per dump: the words of the capture itself.
const DUMP_WORDS: usize = DUMP_BYTES / 8;

/// Where guest memory is known to be readable, as (base, len) pairs.
///
/// **The safety precondition, and the filter, in one.** An argument that is not a pointer
/// is usually a small integer or a length, and dereferencing it would fault *inside the
/// emulator* - turning a diagnostic into a crash with no relation to the guest. Only
/// addresses inside something this process mapped are read, so the dump cannot fault and
/// cannot mistake a count for an address.
///
/// Installed by the layer that does the mapping, because this one must not depend on it -
/// the same inversion the stop handler uses (D160).
static READABLE: std::sync::OnceLock<Box<[(u64, u64)]>> = std::sync::OnceLock::new();

/// Records where guest memory may safely be read from.
pub fn install_readable_ranges(ranges: Vec<(u64, u64)>) {
    let _ = READABLE.set(ranges.into_boxed_slice());
}

/// How many ranges a run can add after the first are published.
///
/// Fixed rather than growable because this is read from the guest's own stack, where allocating
/// is what D381 forbids. **Five hundred and twelve rather than sixty-four**, and the difference
/// was measured: sixty-four was sized for "one per guest thread, and then some", and then guest
/// *mappings* began publishing here too (D579). PPSA03416 reached about a hundred and twenty, so
/// every range past the sixty-fourth was dropped - including the asynchronous-file buffer whose
/// contents are the whole question, which then reported as an address in no published span.
///
/// Eight kibibytes of statics, against a blind spot that reads exactly like a wild pointer
/// (D588).
const MOST_EXTRA_RANGES: usize = 512;

/// Ranges published after the run started, as `(base, len)` pairs.
///
/// A zero length means the slot is empty, which is why a length rather than a base is what
/// marks one used: base zero is a legitimate address to be told about and length zero is
/// not a range.
static EXTRA_RANGES: [(AtomicU64, AtomicU64); MOST_EXTRA_RANGES] =
    [const { (AtomicU64::new(0), AtomicU64::new(0)) }; MOST_EXTRA_RANGES];

/// How many extra ranges have been published.
static EXTRA_COUNT: AtomicU64 = AtomicU64::new(0);

/// How many ranges were published after the table was full.
static DROPPED_RANGES: AtomicU64 = AtomicU64::new(0);

/// How many readable ranges this run could not remember.
///
/// Non-zero means the argument dump has a blind spot whose size is known, which is a different
/// finding from a pointer that is wrong - and the two print identically without this.
#[must_use]
pub fn dropped_ranges() -> u64 {
    DROPPED_RANGES.load(Ordering::Relaxed)
}

/// Publishes a span of guest memory that appeared after the run started.
///
/// # Why the first list is not enough
///
/// The readable ranges are published once, before the guest is entered: the image and the
/// main stack. **A guest thread's stack does not exist yet at that moment**, and every
/// argument a threaded guest passes lives on one - so an argument dump for anything a thread
/// called came back as `no region this run mapped, and address-shaped`, which reads as a wild
/// pointer and is an ordinary stack address.
///
/// That is the diagnostic reporting the wrong kind of thing about its own blind spot, which
/// is principle 3 one level up: `zftpd` serves every client on a thread, so *every* argument
/// worth looking at was invisible, and the tool said "unmapped" rather than "I cannot see
/// there" (D387).
///
/// Allocation-free and lock-free, because a dump runs on the guest's stack (D381).
pub fn note_readable_range(base: u64, len: u64) {
    if len == 0 {
        return;
    }
    let slot = EXTRA_COUNT.fetch_add(1, Ordering::Relaxed);
    let Ok(slot) = usize::try_from(slot) else {
        return;
    };
    let Some((held_base, held_len)) = EXTRA_RANGES.get(slot) else {
        // **Counted, because a dropped range is indistinguishable from a wrong address.** A
        // pointer into a span that overflowed this table reports as "in no span this run
        // published as readable", which is what a wild pointer reports - and the run then has
        // no way to say the difference was capacity (D588).
        DROPPED_RANGES.fetch_add(1, Ordering::Relaxed);
        return;
    };
    held_base.store(base, Ordering::Relaxed);
    // The length last, so a reader never sees a live length against a stale base.
    held_len.store(len, Ordering::Release);
}

/// Whether an address lies in a range published after the run started.
fn in_extra_range(address: u64) -> bool {
    EXTRA_RANGES.iter().any(|(base, len)| {
        let len = len.load(Ordering::Acquire);
        if len == 0 {
            return false;
        }
        let base = base.load(Ordering::Relaxed);
        address >= base && address < base.saturating_add(len)
    })
}

/// What an argument turned out to be.
///
/// # Why "nothing was read" needed splitting in two
///
/// A dump used to record a bool: bytes, or no bytes. So an argument that is a count and an
/// argument that is an **address pointing at nothing this run mapped** rendered
/// identically - as a bare number - and the second is a finding while the first is
/// ordinary.
///
/// That mattered in the worst possible place. The lead on the `image+0xafc959` wall is
/// called with one pointer, and for as long as the readable window was declared a page too
/// low (D217) that pointer read as a count. The tool being used to diagnose the wall was
/// quietly reporting the wrong kind of thing, which is principle 3 exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointing {
    /// Below everything this process mapped for the guest, so not an address at all - a
    /// size, a flag, a count. **Evidence in its own right** (D198).
    Scalar,
    /// Inside a mapped region, and the bytes were read.
    Mapped,
    /// Address-shaped, and no mapped region holds [`DUMP_BYTES`] readable bytes there.
    ///
    /// Either the address is wrong, or the run did not declare the region it points into.
    /// Both are worth saying out loud; neither is a count.
    Unreadable,
}

impl Pointing {
    /// The stored form, so the recording path stays a single relaxed integer store.
    const fn code(self) -> u8 {
        match self {
            Self::Scalar => 0,
            Self::Mapped => 1,
            Self::Unreadable => 2,
        }
    }

    /// Back from the stored form. An unknown code reads as a scalar, which is the claim
    /// that asserts least.
    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Mapped,
            2 => Self::Unreadable,
            _ => Self::Scalar,
        }
    }

    /// Whether bytes were captured.
    pub const fn was_read(self) -> bool {
        matches!(self, Self::Mapped)
    }
}

/// What an argument value is, as far as the mapped regions can say.
///
/// The floor is the lowest base among the ranges this run declared, rather than a constant:
/// the regions are already installed here, and a second copy of where guest memory starts
/// is one more thing that can disagree with the first.
fn classify(address: u64) -> Pointing {
    if is_readable(address) {
        return Pointing::Mapped;
    }
    let floor = READABLE
        .get()
        .and_then(|ranges| ranges.iter().map(|&(base, _)| base).min());
    match floor {
        Some(floor) if address >= floor => Pointing::Unreadable,
        _ => Pointing::Scalar,
    }
}

/// Whether one byte at `address` is inside something this run mapped.
///
/// # The same question the dumper asks, asked by everybody who follows a guest pointer
///
/// The dumper has always checked before dereferencing an argument. Nothing else did: the C
/// library follows a guest pointer because "a guest that passes a bad pointer faults here
/// precisely as it would have faulted there", which is right for a pointer the guest
/// *computed* and wrong for one it never set.
///
/// A `%s` whose argument came out of an overflow area that holds no arguments is the second
/// kind. Dereferencing it crashes inside the renderer, where a report names `vsnprintf` and
/// means something else entirely - and no amount of guarding individual impossible values
/// catches the general case, because the value is arbitrary stack contents (D380).
///
/// One byte rather than [`DUMP_BYTES`], because a string is followed a byte at a time and the
/// question is whether the first one is there.
#[must_use]
pub fn is_mapped(address: u64) -> bool {
    let published = READABLE.get().is_some_and(|ranges| {
        ranges
            .iter()
            .any(|&(base, len)| address >= base && address < base.saturating_add(len))
    });
    // Or a span that appeared after the run started - a guest thread's stack (D387).
    published || in_extra_range(address)
}

/// Whether ranges have been published at all.
///
/// **A run that published none must not have every pointer refused.** The tables are
/// installed by the worker; a unit test, or any caller outside a run, has none - and there
/// the old behaviour is the right one, because there is nothing to check against.
#[must_use]
pub fn ranges_known() -> bool {
    READABLE.get().is_some_and(|ranges| !ranges.is_empty())
}

/// Whether `DUMP_BYTES` from `address` are inside something this process mapped.
fn is_readable(address: u64) -> bool {
    readable_span(address, DUMP_BYTES as u64)
}

/// Whether `[address, address + len)` is inside something this process mapped.
///
/// **The one answer to "may I dereference this?", so there is one place it can be wrong.**
/// The dump asks it about its own fixed window; anything else reading guest memory from
/// outside the guest - a watch, a snapshot - is asking the same question about a length
/// somebody typed, and a second implementation of it is a second chance to fault inside the
/// emulator on an address the guest never touched (D580).
#[must_use]
pub fn readable_span(address: u64, len: u64) -> bool {
    let Some(end) = address.checked_add(len) else {
        return false;
    };
    let published = READABLE.get().is_some_and(|ranges| {
        ranges
            .iter()
            .any(|&(base, range)| address >= base && end <= base.saturating_add(range))
    });
    // The whole window has to be inside one range, published or added later, because the
    // reader reads all of it (D387).
    published || in_extra_range(address) && in_extra_range(end.saturating_sub(1))
}

/// Imports to dump even though something implements them, by symbol index.
///
/// # Why implementing a function must not blind you to it
///
/// Dumps fire for unimplemented imports, on the reasoning that an implemented function's
/// arguments are not a mystery. That reasoning is wrong at exactly the moment it matters:
/// when the implementation is *yours* and you suspect it. `memalign` was implemented in the
/// morning and suspected by the afternoon, and the tool had just stopped being able to show
/// what the guest passed it (D198).
///
/// Opt-in by name rather than always-on, because the busiest import in the corpus is
/// implemented and called tens of millions of times - dumping every implemented call would
/// put an atomic increment on that path for nothing.
static FORCED: std::sync::OnceLock<Box<[bool]>> = std::sync::OnceLock::new();

/// Records which imports to dump regardless of whether they are implemented.
pub fn install_forced_dumps(forced: Vec<bool>) {
    let _ = FORCED.set(forced.into_boxed_slice());
}

/// Whether any import was named for a forced dump.
///
/// When one was, the default set is suppressed: a caller who named an import is asking about that
/// import, and the buffer is small enough that the two compete (D623).
fn anything_forced() -> bool {
    FORCED.get().is_some_and(|f| f.iter().any(|forced| *forced))
}

/// How many dumps were wanted after the buffer was full.
#[must_use]
pub fn dumps_dropped() -> u64 {
    DUMPS_DROPPED.load(Ordering::Relaxed)
}

/// Whether this import is dumped even though it is implemented.
fn is_forced(index: usize) -> bool {
    FORCED
        .get()
        .is_some_and(|f| f.get(index).copied().unwrap_or(false))
}

/// How many dumps have been taken.
static DUMPS_TAKEN: AtomicU64 = AtomicU64::new(0);
/// How many were wanted after the buffer was full.
///
/// The buffer holds [`MAX_DUMPS`] and a guest that calls many unimplemented functions fills it
/// long before an interesting one is reached. Reporting the number is what turns "the tool showed
/// me nothing" into "the tool ran out of room, here is by how much" (D623).
static DUMPS_DROPPED: AtomicU64 = AtomicU64::new(0);
/// Which import each dump belongs to, plus one so zero means empty.
static DUMP_IMPORT: [AtomicU64; MAX_DUMPS] = [const { AtomicU64::new(0) }; MAX_DUMPS];
/// Which argument position was dumped.
static DUMP_SLOT: [AtomicU64; MAX_DUMPS] = [const { AtomicU64::new(0) }; MAX_DUMPS];
/// The address the bytes came from.
static DUMP_ADDRESS: [AtomicU64; MAX_DUMPS] = [const { AtomicU64::new(0) }; MAX_DUMPS];
/// What the argument turned out to be, so a count and an address pointing at nothing are
/// not both shown as an empty buffer.
static DUMP_POINTING: [AtomicU8; MAX_DUMPS] = [const { AtomicU8::new(0) }; MAX_DUMPS];
/// The bytes themselves, as words.
static DUMP_DATA: [[AtomicU64; DUMP_WORDS]; MAX_DUMPS] =
    [const { [const { AtomicU64::new(0) }; DUMP_WORDS] }; MAX_DUMPS];
/// How many calls of each import have been dumped so far.
static DUMPED_PER_IMPORT: std::sync::OnceLock<Box<[AtomicU64]>> = std::sync::OnceLock::new();

/// Prepares the per-import dump counters, alongside the call counters.
pub fn prepare_dumps(count: usize) {
    let mut counters = Vec::with_capacity(count);
    counters.resize_with(count, || AtomicU64::new(0));
    let _ = DUMPED_PER_IMPORT.set(counters.into_boxed_slice());
}

/// What the guest had at one of its pointer arguments when it made a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgumentDump {
    /// Which import was called - an index into the table.
    pub index: u32,
    /// Which argument, counting from zero.
    pub slot: u8,
    /// The address the bytes were read from, which is also the argument's raw value.
    pub address: u64,
    /// What the argument turned out to be.
    ///
    /// **A scalar argument is evidence too.** The first version dumped only arguments that
    /// pointed into mapped memory, so a size, a flag or a count was invisible - and the
    /// question that motivated the whole feature was "what size did the guest ask for?"
    /// (D198).
    ///
    /// It was then a bool, which folded a count together with an address pointing at
    /// nothing this run mapped. See [`Pointing`] for why that mattered (D217).
    pub pointing: Pointing,
    /// The bytes, as they were at the moment of the call.
    pub bytes: [u8; DUMP_BYTES],
}

/// Where a forced write is allowed to land.
///
/// **Deliberately not [`READABLE`].** That list is what may be *read* for a dump, and it
/// includes the image - whose pages are protected after relocation, so a write into a
/// read-only run would fault inside the emulator and produce a crash with no relation to
/// the guest. Only the stack is installed here, which is the region an out-parameter
/// actually lives in.
static WRITABLE: std::sync::OnceLock<Box<[(u64, u64)]>> = std::sync::OnceLock::new();

/// Records where a forced write may safely land. Installed by the layer that maps.
pub fn install_writable_ranges(ranges: Vec<(u64, u64)>) {
    let _ = WRITABLE.set(ranges.into_boxed_slice());
}

/// A value to plant in guest memory before an import answers.
///
/// `Some((slot, value))` writes `value` as a little-endian `u64` at the address in
/// argument `slot`.
#[derive(Clone, Copy, Debug)]
pub struct Plant {
    /// Which argument register holds the pointer to write through.
    pub position: u8,
    /// How far from what that argument points at, in bytes. May be negative.
    pub offset: i64,
    /// The word to store there.
    pub value: u64,
}

/// Every plant configured for one import.
///
/// **A list rather than a single write, and with an offset**, because the question at a
/// wall is *which slot of this structure was the guest waiting to have filled in* - and the
/// first version could only answer it one slot per run, at offset zero. Six runs to
/// eliminate six slots, each one a separate comparison against a separate baseline.
///
/// Given distinct values it is one run: plant a different recognisable number in every
/// candidate, and whichever one the guest uses names itself in what happens next. That is
/// the same reasoning as the self-identifying memory-query fields (D220), pointed at
/// structures the guest passes rather than ones orbistoun fills in (D229).
pub type ForcedWrite = Box<[Plant]>;

/// What to plant for each import, by symbol index.
static FORCED_WRITES: std::sync::OnceLock<Box<[ForcedWrite]>> = std::sync::OnceLock::new();

/// How many forced writes actually landed, and how many were refused.
static WRITES_DONE: AtomicU64 = AtomicU64::new(0);
/// Forced writes that could not be performed because the target was not writable.
static WRITES_REFUSED: AtomicU64 = AtomicU64::new(0);

/// Plants a value in guest memory before an unimplemented import answers.
///
/// # Why this exists at all
///
/// A stub policy can change what a function **answers**. Nothing could change what a
/// function **does** - and both current walls turned out to be a *side effect nobody
/// performed*, not a wrong answer (D217). Worse, the one mechanism for performing a side
/// effect is a `guest_module!` declaration, which is keyed by name, and the function on
/// the biggest wall has no name and no naming source reaches it (D213).
///
/// So the question "is `arg0` an out-parameter the guest expects filled?" was unanswerable
/// by any tool in the project. This answers it, and nothing more: write a recognisable
/// value, run, and see whether the fault address follows it.
///
/// **A diagnostic, not a feature.** Same standing as the stack poison (D185) and forced
/// dumps (D198): driven from the environment because a question is asked once rather than
/// configured, and reported in the run's conditions so a verdict taken under it is never
/// compared with an ordinary one (D218).
/// What the **policy** says a stub writes, by symbol index.
///
/// **The same operation as a forced write, with a different life.** A forced write is a
/// diagnostic - asked once, from the environment, reported in the run's conditions so a
/// verdict under it is never compared with an ordinary run. This is the answer once it is
/// known: data in a file, in force on every run, needing no rebuild (principle 5, D295).
///
/// The value is a **region base the service reserved before the guest started**, not a
/// literal from the file. Reserving address space belongs to the layer that builds it, and
/// certainly not to a trampoline running on the guest's stack under principle 9.
static POLICY_WRITES: std::sync::OnceLock<Box<[ForcedWrite]>> = std::sync::OnceLock::new();

/// Records what each stub writes, from the policy.
pub fn install_policy_writes(writes: Vec<ForcedWrite>) {
    let _ = POLICY_WRITES.set(writes.into_boxed_slice());
}

/// Records a region base each stub answers with, from the policy.
///
/// **The same table a policy answer uses**, because that is what this is: a value the function
/// hands back. Only where it came from differs - the service reserved it before the guest
/// started rather than a person typing it into a file, and a trampoline is the wrong place to
/// be reserving anything (D300).
///
/// Folded into [`install_stub_returns`]' table rather than kept beside it: two tables both
/// answering the same question is how one of them comes to be consulted and the other not.
pub fn install_policy_returns(returns: Vec<(usize, u64)>) {
    let Some(existing) = STUB_RETURNS.get() else {
        // Nothing installed the policy answers yet, so this is the whole table.
        let widest = returns.iter().map(|(slot, _)| *slot).max().unwrap_or(0);
        let mut table = vec![None; widest + 1];
        for (slot, base) in returns {
            if let Some(entry) = table.get_mut(slot) {
                *entry = Some(base);
            }
        }
        let _ = STUB_RETURNS.set(table.into_boxed_slice());
        return;
    };
    // **A region wins over a scalar answer.** A file that says both "answer ok" and "answer a
    // region" for one function is describing one behaviour twice, and the region is the more
    // specific claim - `ok` is what a caller tests, a region is what it uses.
    let mut table = existing.to_vec();
    for (slot, base) in returns {
        if let Some(entry) = table.get_mut(slot) {
            *entry = Some(base);
        }
    }
    OVERRIDDEN_RETURNS.store(table.len() as u64, Ordering::Relaxed);
    let _ = REPLACED_RETURNS.set(table.into_boxed_slice());
}

/// Answers replaced after [`STUB_RETURNS`] was already set.
///
/// A `OnceLock` cannot be set twice, and the region answers are resolved after the scalar ones,
/// so they land here and are consulted first. A second table rather than a silently lost second
/// install, which is the failure this whole family of tables keeps being rescued from.
static REPLACED_RETURNS: std::sync::OnceLock<Box<[Option<u64>]>> = std::sync::OnceLock::new();

/// How many answers the region table replaced.
static OVERRIDDEN_RETURNS: AtomicU64 = AtomicU64::new(0);

/// Records what each stub writes before it answers, from the environment.
///
/// **A diagnostic, not a feature** - the same standing as the stack poison (D185) and forced
/// dumps (D198): driven from the environment because a question is asked once rather than
/// configured, and reported in the run's conditions so a verdict taken under it is never
/// compared with an ordinary one (D218). Once the answer is *known* it belongs in the policy,
/// which is what [`install_policy_writes`] is for.
pub fn install_forced_writes(writes: Vec<ForcedWrite>) {
    let _ = FORCED_WRITES.set(writes.into_boxed_slice());
}

/// How many forced writes landed and how many were refused.
pub fn forced_write_counts() -> (u64, u64) {
    (
        WRITES_DONE.load(Ordering::Relaxed),
        WRITES_REFUSED.load(Ordering::Relaxed),
    )
}

/// Whether a `u64` written at `address` stays inside something installed as writable.
fn is_writable(address: u64) -> bool {
    WRITABLE.get().is_some_and(|ranges| {
        ranges.iter().any(|&(base, len)| {
            address >= base
                && address
                    .checked_add(8)
                    .is_some_and(|end| end <= base.saturating_add(len))
        })
    })
}

/// Performs the forced write configured for `index`, if there is one.
///
/// Refusals are counted rather than ignored. A diagnostic that silently does nothing is
/// indistinguishable from one that ran and changed the answer, and that confusion is the
/// thing this project keeps writing decisions about.
fn forced_write(index: u64, args: *const u64) {
    apply_writes(&FORCED_WRITES, index, args);
    // **The policy's writes, in the same pass and by the same code.** What a stub *does* is
    // the same operation whether a person asked it once from the environment or the loop
    // measured it and wrote it down; only where it comes from and how long it lives differ.
    // A second copy of the store, the bounds check and the refusal counting would be a second
    // place for them to drift (D295).
    apply_writes(&POLICY_WRITES, index, args);
}

/// Performs one table's writes for `index`.
///
/// Refusals are counted rather than ignored. A write that silently did nothing is
/// indistinguishable from one that ran and changed the answer, and that confusion is the
/// thing this project keeps writing decisions about.
fn apply_writes(table: &std::sync::OnceLock<Box<[ForcedWrite]>>, index: u64, args: *const u64) {
    let Some(writes) = table.get() else {
        return;
    };
    let Ok(slot) = usize::try_from(index) else {
        return;
    };
    let Some(plants) = writes.get(slot) else {
        return;
    };
    for plant in plants {
        if usize::from(plant.position) >= SAVED_ARGUMENT_REGISTERS {
            WRITES_REFUSED.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        // SAFETY: the caller guarantees six readable values, and `position` is below six.
        let register = unsafe { args.add(usize::from(plant.position)) };
        // SAFETY: in bounds by the same guarantee, and one word is readable there.
        let pointer = unsafe { register.read() };
        // Wrapped rather than saturated: an offset that ran off the end of the address
        // space would otherwise land on whatever address saturation produced, and
        // `is_writable` would then refuse an address nobody asked about.
        let target = pointer.wrapping_add(plant.offset as u64);
        if !is_writable(target) {
            WRITES_REFUSED.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let Ok(at) = usize::try_from(target) else {
            WRITES_REFUSED.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        // SAFETY: `is_writable` established that eight bytes from `target` lie inside a
        // range this process mapped read-write, so the store is in bounds and cannot fault.
        unsafe {
            std::ptr::write_unaligned(
                std::ptr::with_exposed_provenance_mut::<u64>(at),
                plant.value,
            );
        }
        WRITES_DONE.fetch_add(1, Ordering::Relaxed);
    }
}

/// Captures whatever the guest is pointing at, for a call nothing implements.
///
/// # Why this happens here and not when the trace is collected
///
/// The contents are the point, and they do not survive. A guest passes a stack address, the
/// call returns, and the frame is reused within microseconds - by the time a run is
/// summarised the bytes describe something else entirely. Reading them later would produce
/// a confident, precisely wrong answer, which is worse than none.
///
/// Bounded on every axis so the cost stays where it belongs: only unimplemented imports,
/// only their first calls, only arguments pointing into mapped memory, only a fixed number
/// of dumps in total, and never an allocation (D194).
fn dump_arguments(index: u64, args: *const u64) {
    let Ok(slot) = usize::try_from(index) else {
        return;
    };
    let Some(counters) = DUMPED_PER_IMPORT.get() else {
        return;
    };
    let Some(counter) = counters.get(slot) else {
        return;
    };
    if counter.fetch_add(1, Ordering::Relaxed) >= u64::from(DUMPS_PER_IMPORT) {
        return;
    }

    for position in 0..SAVED_ARGUMENT_REGISTERS {
        // SAFETY: the caller guarantees six readable values, and `position` is below six.
        let slot = unsafe { args.add(position) };
        // SAFETY: in bounds by the same guarantee, and one word is readable there.
        let value = unsafe { slot.read() };
        let pointing = classify(value);
        let readable = pointing.was_read();
        let at = DUMPS_TAKEN.fetch_add(1, Ordering::Relaxed);
        let Ok(at) = usize::try_from(at) else {
            return;
        };
        if at >= MAX_DUMPS {
            // **Counted, not merely refused.** A dump nobody took is indistinguishable from a
            // call that was never made, and the report said nothing - so asking for one import
            // and getting a list without it read as *that import passed no arguments worth
            // showing* (D623).
            DUMPS_DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let from: &[u8] = if readable {
            // SAFETY: this branch is taken only when `is_readable` established that
            // `DUMP_BYTES` from `value` lie inside a range this process mapped, so the
            // whole span is readable.
            unsafe {
                std::slice::from_raw_parts(
                    std::ptr::with_exposed_provenance::<u8>(value as usize),
                    DUMP_BYTES,
                )
            }
        } else {
            &[]
        };
        if readable {
            for (word, slot) in DUMP_DATA[at].iter().enumerate() {
                let mut bits = [0_u8; 8];
                bits.copy_from_slice(&from[word * 8..(word + 1) * 8]);
                slot.store(u64::from_le_bytes(bits), Ordering::Relaxed);
            }
        }
        DUMP_ADDRESS[at].store(value, Ordering::Relaxed);
        DUMP_POINTING[at].store(pointing.code(), Ordering::Relaxed);
        DUMP_SLOT[at].store(position as u64, Ordering::Relaxed);
        // Written last, so a reader never sees a populated import pointing at stale bytes.
        DUMP_IMPORT[at].store(index.wrapping_add(1), Ordering::Relaxed);
    }
}

/// How many of the most recent calls to a named import keep their caller's stack.
///
/// **The most recent, not the first.** Argument dumps keep an import's first calls, because
/// that is when a constant can be told from an out-parameter. The question this answers is
/// *who made the call that went wrong*, and that is usually the last one: PPSA25872's
/// thirty-fifth reservation was the bad one, after thirty-four good ones (worklog 867).
const CALLER_STACKS: usize = 4;

/// Stack words kept per call, from the return address up.
///
/// Four kibibytes: an allocator's frames alone took half a kibibyte in the chain this was built
/// for, and the caller that chose the size sat above them (worklog 867). Return addresses are
/// picked out by the report, which knows where guest code lies; this layer only copies.
pub const CALLER_STACK_WORDS: usize = 512;

/// The ring of captured stacks, and which call each belongs to (sequence, import index + 1).
static CALLER_STACK_DATA: [[AtomicU64; CALLER_STACK_WORDS]; CALLER_STACKS] =
    [const { [const { AtomicU64::new(0) }; CALLER_STACK_WORDS] }; CALLER_STACKS];
static CALLER_STACK_CALL: [(AtomicU64, AtomicU64); CALLER_STACKS] =
    [const { (AtomicU64::new(0), AtomicU64::new(0)) }; CALLER_STACKS];
/// The next ring slot to write.
static CALLER_STACK_NEXT: AtomicU64 = AtomicU64::new(0);

/// Copies the caller's stack, from its return address up, for a call to an import named with
/// `ORBISTOUN_DUMP`.
///
/// **An observation, not a walk.** No frame pointer is followed, so a function that keeps none
/// costs nothing here; the words are copied as they are and the report says which of them are
/// addresses in guest code. A stale word from an older frame can look like a return address, and
/// the report says so rather than presenting the list as a call chain.
///
/// Only a span [`readable_span`] vouches for is read, so this cannot fault inside the emulator;
/// allocation-free, because it runs on the guest's stack (D381).
fn capture_caller_stack(sequence: u64, index: u64, entry_rsp: u64) {
    let bytes = (CALLER_STACK_WORDS * 8) as u64;
    if entry_rsp == 0 || !readable_span(entry_rsp, bytes) {
        return;
    }
    let slot = (CALLER_STACK_NEXT.fetch_add(1, Ordering::Relaxed) as usize) % CALLER_STACKS;
    // Cleared first so a reader never pairs this call with the previous occupant's words.
    CALLER_STACK_CALL[slot].1.store(0, Ordering::Relaxed);
    for (word, cell) in CALLER_STACK_DATA[slot].iter().enumerate() {
        let at = entry_rsp + (word as u64) * 8;
        // SAFETY: `readable_span` established that `bytes` from `entry_rsp` lie inside a range
        // this process mapped, and `at` is within that span and eight-byte aligned relative to
        // it, so the unaligned read of one word is in bounds and cannot fault.
        let value = unsafe {
            std::ptr::read_unaligned(std::ptr::with_exposed_provenance::<u64>(at as usize))
        };
        cell.store(value, Ordering::Relaxed);
    }
    CALLER_STACK_CALL[slot].0.store(sequence, Ordering::Relaxed);
    // Written last: a populated marker always has its words behind it.
    CALLER_STACK_CALL[slot]
        .1
        .store(index.wrapping_add(1), Ordering::Relaxed);
}

/// A captured caller stack: which call it was, and the words from its return address up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerStack {
    /// The call's position in the global order.
    pub sequence: u64,
    /// Which import was called - an index into the table.
    pub index: u32,
    /// The stack as the guest left it at the call, word 0 being the return address.
    pub words: Vec<u64>,
}

/// The caller stacks captured this run, oldest first.
#[must_use]
pub fn caller_stacks() -> Vec<CallerStack> {
    let mut out: Vec<CallerStack> = CALLER_STACK_CALL
        .iter()
        .zip(&CALLER_STACK_DATA)
        .filter_map(|((sequence, marker), data)| {
            let index = marker.load(Ordering::Relaxed).checked_sub(1)?;
            Some(CallerStack {
                sequence: sequence.load(Ordering::Relaxed),
                index: u32::try_from(index).unwrap_or(u32::MAX),
                words: data.iter().map(|w| w.load(Ordering::Relaxed)).collect(),
            })
        })
        .collect();
    out.sort_by_key(|stack| stack.sequence);
    out
}

/// Every argument dump taken this run.
pub fn argument_dumps() -> Vec<ArgumentDump> {
    let mut out = Vec::new();
    for at in 0..MAX_DUMPS {
        let marker = DUMP_IMPORT[at].load(Ordering::Relaxed);
        let Some(index) = marker.checked_sub(1) else {
            continue;
        };
        let mut bytes = [0_u8; DUMP_BYTES];
        for word in 0..DUMP_WORDS {
            let bits = DUMP_DATA[at][word].load(Ordering::Relaxed).to_le_bytes();
            bytes[word * 8..(word + 1) * 8].copy_from_slice(&bits);
        }
        out.push(ArgumentDump {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            slot: u8::try_from(DUMP_SLOT[at].load(Ordering::Relaxed)).unwrap_or(u8::MAX),
            address: DUMP_ADDRESS[at].load(Ordering::Relaxed),
            pointing: Pointing::from_code(DUMP_POINTING[at].load(Ordering::Relaxed)),
            bytes,
        });
    }
    out
}

/// One recorded call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedCall {
    /// Position in the global call order, starting at zero.
    pub sequence: u64,
    /// Which import was called - an index into the table this thunk belongs to.
    pub index: u32,
    /// The call's integer arguments, in register order - `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.
    ///
    /// **All six, because a size is rarely the first one.** See `RING_ARGS` for what recording
    /// only the first cost.
    pub args: [u64; SAVED_ARGUMENT_REGISTERS],
    /// Which host thread made it, or zero when nothing recorded one.
    ///
    /// Only ever compared for equality - two calls with the same value happened on the same
    /// thread, and that is the whole question it answers.
    pub thread: u64,
    /// The guest address this call returns to - one instruction past the call site.
    ///
    /// Zero when it could not be read. Matches the addresses a fault's frame walk reports,
    /// which is what lets a stack trace and a call trace be read against each other.
    pub from: u64,
    /// What this call **answered** in `rax`, or [`None`] if it had not returned yet when the
    /// record was read - a call still running, or one whose guest faulted the instant it
    /// returned. `None` is not zero: zero is a real answer (`OK`) and the commonest one.
    pub ret: Option<u64>,
}

/// Prepares per-import counters and argument-shape slots for a table of `count` entries.
///
/// Called once, at table construction. Doing it here rather than lazily is what keeps
/// the call path allocation-free.
pub fn prepare_counters(count: usize) {
    let _ = COUNTS.set((0..count).map(|_| AtomicU64::new(0)).collect());
    let _ = SHAPES.set(
        (0..count * SAVED_ARGUMENT_REGISTERS)
            .map(|_| AtomicU8::new(0))
            .collect(),
    );
}

/// Total calls the guest has made through any thunk.
pub fn total_calls() -> u64 {
    SEQUENCE.load(Ordering::Relaxed)
}

/// How many times each import was called, by index.
pub fn call_counts() -> Vec<u64> {
    COUNTS.get().map_or_else(Vec::new, |c| {
        c.iter().map(|n| n.load(Ordering::Relaxed)).collect()
    })
}

/// The [shape bits](SHAPE_ZERO) inferred for each import's arguments, by index - one
/// `[u8; SAVED_ARGUMENT_REGISTERS]` per import, parallel to [`call_counts`].
///
/// Each slot is the OR of the categories that argument carried across the sampled calls (the
/// first `SHAPE_SAMPLE_LIMIT` of them): `0` means the slot was never written - either the import
/// was never called or it takes fewer arguments. This is the guest describing its own imports by
/// how it uses them, the only signature source a black box allows.
pub fn arg_shapes() -> Vec<[u8; SAVED_ARGUMENT_REGISTERS]> {
    SHAPES.get().map_or_else(Vec::new, |s| {
        s.chunks_exact(SAVED_ARGUMENT_REGISTERS)
            .map(|chunk| {
                let mut shape = [0u8; SAVED_ARGUMENT_REGISTERS];
                for (out, slot) in shape.iter_mut().zip(chunk) {
                    *out = slot.load(Ordering::Relaxed);
                }
                shape
            })
            .collect()
    })
}

/// The answer a recorded call handed back, or [`None`] if it had not returned when read.
///
/// Reads the flag with `Acquire` against the `Release` the recording store used, so a seen
/// flag guarantees the answer beside it is the one that belongs to it.
fn recorded_return(slot: usize) -> Option<u64> {
    (RING_RETURNED[slot].load(Ordering::Acquire) == 1)
        .then(|| RING_RET[slot].load(Ordering::Relaxed))
}

/// The call the guest most recently entered, if any.
///
/// **Allocation-free on purpose.** The caller is a fault handler on a thread that has just
/// faulted, and the one question it needs answered is "what was running?" - which
/// `recorded_calls` can also answer, but only by building a vector of everything.
///
/// It reads the ring rather than a separate "current" slot, so it cannot disagree with the
/// trace. A call still being recorded reads as the one before it, which is the honest
/// answer: that call had definitely started.
pub fn last_call() -> Option<RecordedCall> {
    // **The highest sequence, not the highest slot.** While the ring filled once those were the
    // same; wrapped, the newest call can sit anywhere in the buffer, and taking the last slot
    // would name whichever function happened to land there (D571).
    let newest = (0..MAX_RECORDED_CALLS)
        .filter(|i| RING[*i].load(Ordering::Relaxed) != 0)
        .max_by_key(|i| RING_SEQ[*i].load(Ordering::Relaxed))?;
    let index = RING[newest].load(Ordering::Relaxed).checked_sub(1)?;
    Some(RecordedCall {
        sequence: RING_SEQ[newest].load(Ordering::Relaxed).saturating_sub(1),
        index: index as u32,
        args: std::array::from_fn(|r| {
            RING_ARGS[newest * SAVED_ARGUMENT_REGISTERS + r].load(Ordering::Relaxed)
        }),
        from: RING_FROM[newest].load(Ordering::Relaxed),
        thread: RING_THREAD[newest].load(Ordering::Relaxed),
        ret: recorded_return(newest),
    })
}

/// Which import **this thread** is currently inside, if any.
///
/// # Why [`last_call`] is the wrong question for a caller inside a call
///
/// `last_call` reads the whole ring and answers with the newest call *any* thread made. That is
/// right for a fault handler asking "what was this process doing", and wrong for an
/// implementation asking "what am I inside" - which is what the mapping record and the format
/// trace both ask.
///
/// The consequence was visible and read as ordinary: the mapping list attributed reservations to
/// `libc::memcpy`, which maps nothing. A guest thread reserving memory was labelled with whatever
/// another thread had most recently entered, and the two are indistinguishable in the output
/// (D616).
///
/// Zero means *not inside one*, so the slot needs no initialiser beyond zero and a thread that
/// never entered a call answers honestly rather than naming somebody else's.
///
/// **A stack, because calls nest.** A guest call can re-enter through a callback, and restoring
/// the previous value on the way out is what keeps the outer call's identity for the rest of its
/// body. One `u32` in thread-local storage, written twice per call, which is the whole cost.
pub fn current_call() -> Option<u32> {
    INSIDE.with(|inside| inside.get().checked_sub(1))
}

/// This host thread's own identifier, as the recorded calls carry it.
///
/// Exposed so a fault can be paired with the calls that happened on the same thread - the fault
/// handler runs on the faulting thread, so it reads its own (D621).
#[must_use]
pub fn current_thread() -> u64 {
    host_thread()
}

/// This host thread's own identifier, cheap and stable for the thread's life.
///
/// The address of a thread-local byte: distinct per thread, constant within one, and costing a
/// load rather than a system call - which matters because this runs on every guest call and
/// recording must not change what it observes (principle 9).
/// A stable identifier for the calling host thread.
///
/// The address of a thread-local marker: unique per thread, free to read, and never zero.
/// Public so the thread registry can record which host thread a guest handle is running on -
/// without it the recorded calls and the thread table cannot be joined, and a report can say
/// which threads exist or what was called last but never both (D651).
#[must_use]
pub fn host_thread() -> u64 {
    thread_local! {
        static MARK: u8 = const { 0 };
    }
    MARK.with(|m| std::ptr::from_ref(m) as u64)
}

thread_local! {
    /// The import this thread is inside, plus one. Zero means none.
    ///
    /// Thread-local rather than an array indexed by thread handle: a guest thread has no small
    /// dense identifier here, and recording must not allocate (principle 9).
    static INSIDE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
/// The most recent calls, in the order the guest made them.
///
/// At most [`MAX_RECORDED_CALLS`] of them; [`total_calls`] says how many there were altogether.
///
/// **Ordered by the recorded sequence, not by slot.** The ring wraps, so slot order is the order
/// the buffer happens to sit in and says nothing about the guest (D571).
pub fn recorded_calls() -> Vec<RecordedCall> {
    let mut out: Vec<RecordedCall> = (0..MAX_RECORDED_CALLS)
        .filter_map(|i| {
            let stored = RING[i].load(Ordering::Relaxed);
            // Zero means the slot was claimed but not yet written - another thread is
            // mid-record. Skipped rather than reported as import zero, which is a real
            // index and would be a lie.
            let sequence = RING_SEQ[i].load(Ordering::Relaxed).checked_sub(1)?;
            stored.checked_sub(1).map(|index| RecordedCall {
                sequence,
                index: index as u32,
                args: std::array::from_fn(|r| {
                    RING_ARGS[i * SAVED_ARGUMENT_REGISTERS + r].load(Ordering::Relaxed)
                }),
                from: RING_FROM[i].load(Ordering::Relaxed),
                thread: RING_THREAD[i].load(Ordering::Relaxed),
                ret: recorded_return(i),
            })
        })
        .collect();
    out.sort_unstable_by_key(|c| c.sequence);
    out
}

/// The indices of the run's first calls, in order, however long it ran.
///
/// Kept apart from the ring so the circular one can answer *what did it call last* without
/// costing the other question - the halt summary quotes these (D571).
pub fn opening_calls() -> Vec<u32> {
    RING_OPENING
        .iter()
        .filter_map(|slot| slot.load(Ordering::Relaxed).checked_sub(1))
        .map(|index| index as u32)
        .collect()
}

/// The run's first calls with the address each was made from, in order.
///
/// **The sequence number is the position**, so the caller gets *call 219 was this* for free -
/// which is what makes the record diffable against the mapping record, whose entries are indexed
/// by call ordinal (D581, D603).
///
/// Stops at the first empty slot rather than filtering, because a gap would shift every later
/// call's number and the numbers are the whole point.
#[must_use]
pub fn opening_sequence() -> Vec<(u64, u32, u64)> {
    RING_OPENING
        .iter()
        .zip(RING_OPENING_FROM.iter())
        .enumerate()
        .map_while(|(position, (slot, from))| {
            let index = slot.load(Ordering::Relaxed).checked_sub(1)?;
            Some((position as u64, index as u32, from.load(Ordering::Relaxed)))
        })
        .collect()
}

/// Records one guest call and answers it.
///
/// `args` points at the six argument registers spilled by [`trampoline`], in System V
/// order. Reading past the sixth is out of bounds - the seventh argument onwards is on
/// the guest stack and is not captured here.
///
/// `entry_rsp` is the stack pointer as the trampoline first saw it, before it touched
/// anything - which is exactly what the guest's `call` left behind, and therefore the one
/// number that says whether the guest obeys the calling convention (D159).
///
/// # Safety
///
/// `args` must point to [`SAVED_ARGUMENT_REGISTERS`] readable `u64` values. The
/// trampoline is the only caller and satisfies this by construction.
unsafe extern "sysv64" fn on_guest_call(
    index: u64,
    args: *const u64,
    entry_rsp: u64,
    floats: *mut u64,
) -> u64 {
    // **Where a backgrounded title actually stops.** This is the one place every guest call
    // passes through, and a thread that stops *here* is in our code holding no guest lock -
    // unlike one frozen at an arbitrary instruction, which may hold the host heap lock and
    // deadlock the whole worker including whatever would have resumed it (D344).
    //
    // Before the sequence number, so a parked call is not counted as having happened yet.
    orbistoun_core::park::check();

    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);

    // Checked before anything else this call would do, so the run stops at exactly the
    // budgeted call rather than somewhere near it. A watcher thread polling the counter
    // would have cost nothing here and stopped at "about" N, which is the nondeterminism
    // this exists to remove (D238).
    if sequence >= CALL_BUDGET.load(Ordering::Relaxed) {
        if let Some(stop) = ON_BUDGET.get() {
            stop();
        }
    }

    record_alignment(sequence, index, entry_rsp);

    if let Some(counts) = COUNTS.get() {
        if let Some(counter) = counts.get(index as usize) {
            // `fetch_add` hands back the count *before* this call, so the first
            // `SHAPE_SAMPLE_LIMIT` calls of each import contribute their argument shapes and the
            // millions after them pay only the increment - the shape is settled long before the
            // limit (principle 9).
            let seen = counter.fetch_add(1, Ordering::Relaxed);
            if seen < SHAPE_SAMPLE_LIMIT {
                if let Some(shapes) = SHAPES.get() {
                    let base = index as usize * SAVED_ARGUMENT_REGISTERS;
                    for register in 0..SAVED_ARGUMENT_REGISTERS {
                        // SAFETY: the caller guarantees `SAVED_ARGUMENT_REGISTERS` readable values
                        // in register order, and `register` is bounded by that count - so the
                        // offset stays inside the array the caller provided.
                        let at = unsafe { args.add(register) };
                        // SAFETY: `at` is inside the caller's array, per the block above, and
                        // these are plain integers - no alignment or validity demand beyond
                        // being readable.
                        let value = unsafe { at.read() };
                        if let Some(slot) = shapes.get(base + register) {
                            slot.fetch_or(classify_arg(value), Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    if let Ok(position) = usize::try_from(sequence) {
        // The opening, kept whole and never overwritten - a separate, tiny record so that making
        // the main ring circular did not cost the other question (D571).
        if let Some(opening) = RING_OPENING.get(position) {
            // The call site first, so a reader that sees a populated index never finds a stale
            // address beside it - the same ordering the main ring uses three lines below.
            if let Some(from) = RING_OPENING_FROM.get(position) {
                from.store(call_site(entry_rsp), Ordering::Relaxed);
            }
            opening.store(index + 1, Ordering::Relaxed);
        }
        {
            let slot = position % MAX_RECORDED_CALLS;
            for register in 0..SAVED_ARGUMENT_REGISTERS {
                // SAFETY: the caller guarantees `SAVED_ARGUMENT_REGISTERS` readable values in
                // register order, and `register` is bounded by that count - so the offset stays
                // inside the array the caller provided.
                let at = unsafe { args.add(register) };
                // SAFETY: `at` is inside the caller's array, per the block above, and these are
                // plain integers - no alignment or validity demand beyond being readable.
                let value = unsafe { at.read() };
                RING_ARGS[slot * SAVED_ARGUMENT_REGISTERS + register]
                    .store(value, Ordering::Relaxed);
            }
            RING_FROM[slot].store(call_site(entry_rsp), Ordering::Relaxed);
            RING_THREAD[slot].store(host_thread(), Ordering::Relaxed);
            // **The previous occupant's answer is retired before this call claims the slot.**
            // Nothing cleared it, so a call still running in a recycled slot reported the answer
            // of whichever call held it last - and `recorded_return`'s whole contract is that a
            // call which has not returned reads as unknown.
            //
            // Invisible while every call returned promptly, because the window was a few
            // instructions wide. `sceKernelWaitEqueue` learning to block held one slot open for
            // the length of a wait, and the trace tail started reporting a mapping address as
            // the answer to an event-queue wait (D613).
            //
            // Ordered before the sequence with `Release`, so a reader that sees this call's
            // number can never still see the last call's flag.
            RING_RETURNED[slot].store(0, Ordering::Release);
            RING_SEQ[slot].store(sequence.wrapping_add(1), Ordering::Relaxed);
            // Written after the argument and the sequence, so a reader never sees a populated
            // index pointing at a stale argument or at the wrong call's number.
            RING[slot].store(index.wrapping_add(1), Ordering::Relaxed);
        }
    }

    // Only for calls nothing implements: an implemented function's arguments are not a
    // mystery, and skipping them is what keeps this off the hot path entirely - the
    // busiest import in the corpus is called ninety-nine million times and is implemented
    // (D194).
    // Looked up **once**, and the dump decided from the result. Asking `is_implemented`
    // separately cost a second table lookup on every call including the implemented ones,
    // and the busiest title in the corpus makes sixty-eight million of those - a
    // measurable slowdown, which is a sink changing the program it observes (principle 9).
    let handler = HANDLERS
        .get()
        .and_then(|h| h.get(index as usize))
        .and_then(|h| *h);

    // Only for calls nothing implements: an implemented function's arguments are not a
    // mystery, and skipping them is what keeps this off the hot path entirely (D194).
    //
    // **A forced list narrows the dump to itself.** It used to add to the default set, and on a
    // guest that calls hundreds of unimplemented functions the buffer filled with those before
    // the forced one was reached - so asking a specific question returned an answer to a
    // different one. Somebody who names an import is asking about that import (D623).
    if is_forced(index as usize) || (handler.is_none() && !anything_forced()) {
        dump_arguments(index, args);
    }
    if is_forced(index as usize) {
        capture_caller_stack(sequence, index, entry_rsp);
    }

    // After the dump, deliberately: the dump must record what the guest passed, not what
    // this planted. Reversing them would make the experiment invisible in its own evidence.
    forced_write(index, args);

    // The dispatch is split out so every possible answer - a float result, an integer
    // result, a forced diagnostic value, a stub's placeholder - leaves through one point,
    // which is where the return below is recorded. `handler` is handed over rather than
    // looked up again, keeping the "looked up once" guarantee above.
    //
    // **Set around the call, restored after.** An implementation asking what it is inside gets
    // its own thread's answer rather than the newest call any thread made (D616). It brackets
    // exactly the handler, which is the span the question is about.
    let outer = INSIDE.with(|inside| inside.replace((index as u32).wrapping_add(1)));
    // SAFETY: `args`, `floats` and `entry_rsp` are this function's own parameters, forwarded
    // unchanged, so they still satisfy the contract the trampoline established.
    let answer = unsafe { resolve(index, args, entry_rsp, handler, floats) };
    INSIDE.with(|inside| inside.set(outer));

    // The other half of the record, and the half nothing captured until now: what the call
    // answered. Written *after* the handler ran, so a slot read before then reads as
    // *unknown* rather than as this slot's initial zero - which `OK` also is (D459).
    if let Ok(position) = usize::try_from(sequence) {
        // The same slot the call went into, and **only if it is still that call's**: after a wrap
        // a later call owns the slot, and writing this answer there would attribute it to the
        // wrong function - the one failure a circular ring can cause that a filling one cannot
        // (D571).
        let slot = position % MAX_RECORDED_CALLS;
        if RING_SEQ[slot].load(Ordering::Relaxed) == sequence.wrapping_add(1) {
            RING_RET[slot].store(answer, Ordering::Relaxed);
            // Release, paired with the Acquire in `recorded_return`, so seeing the flag set
            // guarantees the answer beside it is this call's and not the stale initial word.
            RING_RETURNED[slot].store(1, Ordering::Release);
        }
    }

    answer
}

/// Dispatches one already-recorded call to whatever answers it and returns what goes back to
/// the guest in `rax` (and, for a float function, `xmm0` via `floats`).
///
/// Split out of [`on_guest_call`] so that every answer leaves through one point - which is
/// where the caller records the return. `handler` is passed rather than resolved again: the
/// caller looked it up once to decide whether to dump, and a second lookup on this path is a
/// cost the busiest import in the corpus would pay ninety-nine million times (principle 9).
///
/// # Safety
///
/// Same contract as [`on_guest_call`]: `args` points at [`SAVED_ARGUMENT_REGISTERS`] readable
/// `u64` values and `floats` at [`orbistoun_core::GUEST_FLOAT_REGISTERS`] writable ones, both
/// outliving the call.
unsafe fn resolve(
    index: u64,
    args: *const u64,
    entry_rsp: u64,
    handler: Option<GuestFn>,
    floats: *mut u64,
) -> u64 {
    // Before the integer handler, because a function that answers in `xmm0` has nothing
    // useful to say in `rax` and the two tables are disjoint by construction (D268).
    if let Some(float_handler) = FLOAT_HANDLERS
        .get()
        .and_then(|h| h.get(index as usize))
        .and_then(|h| *h)
    {
        // SAFETY: the trampoline spilled six integer argument registers to the stack
        // immediately below this frame, and the array outlives the call.
        let ints = unsafe { &*args.cast::<[u64; SAVED_ARGUMENT_REGISTERS]>() };
        // SAFETY: and eight floating-point ones below those, by the same spill.
        let float_args = unsafe { &*floats.cast::<[u64; orbistoun_core::GUEST_FLOAT_REGISTERS]>() };
        let previous = overflow::begin(entry_rsp);
        let answer = float_handler(ints, float_args);
        overflow::end(previous);
        // Written where the trampoline will load `xmm0` from.
        // SAFETY: the same eight-slot array, which is writable and this thread's own.
        unsafe { floats.write(answer) };
        // `rax` too, so a function whose result is read as an integer somewhere is not
        // handed a stale one.
        return answer;
    }

    if let Some(handler) = handler {
        // SAFETY: the caller guarantees six readable values, which is the array this
        // reborrows. The trampoline spilled them and they outlive this call.
        let args: &[u64; SAVED_ARGUMENT_REGISTERS] = unsafe { &*args.cast() };
        // Published for the length of the call, so a variadic implementation can read the
        // arguments that did not fit in registers (D385).
        let previous = overflow::begin(entry_rsp);
        let answer = handler(args);
        overflow::end(previous);
        // **The implementation still runs; only its answer is replaced.** Skipping it
        // would suppress every side effect too, and a diagnostic asking "is this the
        // wrong answer?" wants the rest of the program to behave exactly as it did -
        // otherwise a moved fault says only that the program was changed, which is
        // already known (D234).
        //
        // The `get()` is one atomic load that short-circuits to `None` on any ordinary
        // run, so the busiest import in the corpus - implemented, ninety-nine million
        // calls - pays a predictable branch and no table lookup (principle 9).
        if let Some(value) = forced_answer(index) {
            return value;
        }
        return answer;
    }

    // What an unimplemented function answers depends on what kind of value it returns.
    // For anything the caller *dereferences*, an error code is a wild pointer - so those
    // answer zero, which is what a caller already tests for (D125).
    // Before the policy answer, so a diagnostic reaches a function whose answer the
    // policy already sets - and counted, so a forced return that matched nothing is
    // visible rather than inferred from an unchanged run (D230).
    if let Some(value) = forced_answer(index) {
        return value;
    }

    // Regions first: they are resolved after the scalar answers and cannot overwrite a
    // `OnceLock`, so they live in their own table and win where both have an entry (D300).
    if let Some(value) = REPLACED_RETURNS
        .get()
        .and_then(|v| v.get(index as usize))
        .and_then(|v| *v)
    {
        return value;
    }
    if let Some(value) = STUB_RETURNS
        .get()
        .and_then(|v| v.get(index as usize))
        .and_then(|v| *v)
    {
        return value;
    }

    // Otherwise: never zero, which a guest would read as success. An explicit "not
    // handled" is worth more than a wrong answer and costs the same (principle 3).
    //
    // Widened rather than truncated: guest error codes are 32-bit and a caller reading
    // the full register must not see whatever the upper half happened to hold.
    u64::from(GuestError::Unimplemented.as_raw())
}

/// The address every thunk jumps to.
pub fn trampoline_address() -> u64 {
    // Named with its full type before casting: a bare `as usize` on a function item is
    // a different, easier-to-get-wrong conversion, and clippy is right to object.
    let f: unsafe extern "sysv64" fn() = trampoline;
    f as usize as u64
}

/// Hand-written entry point shared by every thunk.
///
/// Spills the six System V argument registers to the stack, presents them as an
/// ordinary two-argument call, and passes the handler's return value straight back to
/// the guest in `rax`.
///
/// `sub rsp, 8` corrects the alignment a `call` left odd; the six pushes then move 48
/// bytes, which preserves it. `add rsp, 56` undoes both before returning.
///
/// Naked because every instruction here matters: a prologue the compiler inserted would
/// clobber the argument registers before they are saved.
#[unsafe(naked)]
unsafe extern "sysv64" fn trampoline() {
    core::arch::naked_asm!(
        // Before anything else: what the guest's `call` left behind. `r11` is scratch
        // under System V and is already dead here - the thunk used it to hold this
        // address and jumped through it - so it is the one register that can carry a
        // value across the spill without destroying an argument (D159).
        "mov r11, rsp",
        "sub rsp, 8",
        "push r9",
        "push r8",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        // The eight floating-point argument registers, low halves - a `double`, and a
        // `float` in its low half. Spilled unconditionally, which is the cost of not
        // having half an ABI: a maths function's argument arrives *only* here, so a
        // trampoline that skips them hands the implementation six integer registers that
        // do not contain it (D268).
        //
        // Eight stores against six pushes and a call that already happen. If it ever
        // measures badly the answer is a second trampoline, chosen per import at table
        // build time, so the integer path pays nothing - the stubs already carry their
        // own trampoline address.
        "sub rsp, 64",
        "movsd [rsp], xmm0",
        "movsd [rsp + 8], xmm1",
        "movsd [rsp + 16], xmm2",
        "movsd [rsp + 24], xmm3",
        "movsd [rsp + 32], xmm4",
        "movsd [rsp + 40], xmm5",
        "movsd [rsp + 48], xmm6",
        "movsd [rsp + 56], xmm7",
        "mov rdi, r10",
        // The integer array sits above the floating-point one now.
        "lea rsi, [rsp + 64]",
        // Third argument: the incoming stack pointer. Safe to clobber `rdx` here - the
        // guest's value in it was spilled by the push above, so the handler reads it from
        // the array rather than the register.
        "mov rdx, r11",
        // Fourth: the floating-point array, which the handler also writes its answer into.
        "mov rcx, rsp",
        "call {handler}",
        // Whatever the handler left in the first slot becomes `xmm0`. For an ordinary
        // integer function that is the value the guest passed in, written straight back -
        // which is what the register held anyway, so nothing is disturbed.
        "movsd xmm0, [rsp]",
        "add rsp, 120",
        "ret",
        handler = sym on_guest_call,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RECORDED_CALLS, RING, RING_SEQ, SAVED_ARGUMENT_REGISTERS, implemented_count,
        install_float_handlers, is_implemented, trampoline_address,
    };
    use std::sync::atomic::Ordering;

    /// **A slot's sequence is stored offset by one, so an untouched slot is empty rather than
    /// call zero.**
    ///
    /// The same convention `RING` uses for the index, and for the same reason: zero is a real
    /// call number, so a slot nothing has written must be distinguishable from the first call.
    #[test]
    fn an_untouched_slot_reads_as_empty_rather_than_as_call_zero() {
        // A slot far past anything these tests exercise.
        let untouched = MAX_RECORDED_CALLS - 1;
        assert_eq!(
            RING_SEQ[untouched].load(Ordering::Relaxed).checked_sub(1),
            None,
            "an unwritten sequence must not decode to call zero"
        );
        assert_eq!(
            RING[untouched].load(Ordering::Relaxed).checked_sub(1),
            None,
            "nor an unwritten index to import zero"
        );
    }

    /// **Each argument value lands in exactly the category its magnitude names.**
    ///
    /// The boundaries are the whole point of the inference: a value one below the scalar ceiling
    /// is a scalar, the ceiling itself is not; the guest's address space is a pointer, one past
    /// its top is not. Pure by design so the edges are pinned without a running guest - made to
    /// fail by asserting the category, not that *some* bit is set.
    #[test]
    fn an_argument_is_classified_by_where_its_value_falls() {
        use super::{SHAPE_OTHER, SHAPE_POINTER, SHAPE_SCALAR, SHAPE_ZERO, classify_arg};

        assert_eq!(classify_arg(0), SHAPE_ZERO, "zero is unused, not a scalar");
        assert_eq!(classify_arg(1), SHAPE_SCALAR, "a small count is a scalar");
        assert_eq!(
            classify_arg(0xffff),
            SHAPE_SCALAR,
            "the value just below the ceiling is still a scalar"
        );
        assert_eq!(
            classify_arg(0x1_0000),
            SHAPE_OTHER,
            "the ceiling itself is above the scalar band but below any address"
        );
        assert_eq!(
            classify_arg(0x4000_0000_0000),
            SHAPE_POINTER,
            "the image base is the bottom of the guest address space"
        );
        assert_eq!(
            classify_arg(0x7fff_ffff_f000),
            SHAPE_POINTER,
            "a guest pool address is a pointer"
        );
        assert_eq!(
            classify_arg(0x8000_0000_0000),
            SHAPE_OTHER,
            "one past the top of the address space is not a pointer"
        );
    }

    /// **A slot that is sometimes a pointer and sometimes zero reads as an optional pointer.**
    ///
    /// The accumulator ORs each call's category into the slot, so the inference is the union of
    /// what the argument has ever been - which is exactly how an optional pointer (`ptr` on some
    /// calls, `NULL` on others) must present. Pins the OR semantics the reader depends on.
    #[test]
    fn shapes_or_together_across_calls_into_an_optional_pointer() {
        use super::{SHAPE_POINTER, SHAPE_ZERO, classify_arg};

        let across_calls = classify_arg(0) | classify_arg(0x4000_0010_0000);
        assert_eq!(
            across_calls,
            SHAPE_ZERO | SHAPE_POINTER,
            "a slot seen as both NULL and a pointer is an optional pointer, not one or the other"
        );
    }

    /// **The rendered signature reports arity and marks nullable slots.**
    ///
    /// Trailing untouched registers are dropped so the parentheses say how many arguments the
    /// guest actually passed; a slot that was also zero on some call is marked `?`. Made to fail
    /// by pinning the exact string, so a change to arity handling or the nullable mark shows.
    #[test]
    fn a_shape_renders_as_a_signature_with_arity_and_nullable_marks() {
        use super::{
            SAVED_ARGUMENT_REGISTERS, SHAPE_OTHER, SHAPE_POINTER, SHAPE_SCALAR, SHAPE_ZERO,
            describe_shape,
        };

        // arg0 always a pointer, arg1 pointer-or-null, arg2 a scalar; arg3-5 never written.
        let mut shape = [0u8; SAVED_ARGUMENT_REGISTERS];
        shape[0] = SHAPE_POINTER;
        shape[1] = SHAPE_POINTER | SHAPE_ZERO;
        shape[2] = SHAPE_SCALAR;
        assert_eq!(
            describe_shape(&shape),
            "(ptr, ptr?, u32)",
            "three args, the middle one nullable, trailing untouched registers dropped"
        );

        assert_eq!(
            describe_shape(&[0u8; SAVED_ARGUMENT_REGISTERS]),
            "()",
            "an import called with no arguments renders empty parentheses"
        );

        // A register the guest always passed as zero is within arity, not dropped.
        let mut always_zero = [0u8; SAVED_ARGUMENT_REGISTERS];
        always_zero[0] = SHAPE_SCALAR;
        always_zero[1] = SHAPE_ZERO;
        assert_eq!(
            describe_shape(&always_zero),
            "(u32, 0)",
            "an always-zero argument is a present slot, distinct from an unwritten one"
        );

        // A large non-address value reads as u64, not a pointer.
        let mut wide = [0u8; SAVED_ARGUMENT_REGISTERS];
        wide[0] = SHAPE_OTHER;
        assert_eq!(describe_shape(&wide), "(u64)", "a large non-address is u64");
    }

    /// A function answering in `xmm0` counts as implemented.
    ///
    /// **The failure this fixes reported success as absence.** `is_implemented` asked only the
    /// integer table, so every maths function dispatched correctly, computed the right answer,
    /// passed its conformance check - and was recorded as a call nothing implemented. It took
    /// argument dumps for functions whose integer registers hold nothing but leftovers, ranked
    /// finished work to the top of the findings list, and understated `standing`, which is the
    /// number this project reads as its own progress (D268, D290).
    ///
    /// One test rather than several: the tables are process-global and set once, so a second
    /// test installing its own would race this one.
    #[test]
    fn a_function_answering_in_a_float_register_is_implemented() {
        /// Stands in for a maths function. What it returns does not matter; that it is
        /// *attached* does.
        fn answers(
            _ints: &[u64; orbistoun_core::GUEST_ARG_REGISTERS],
            _floats: &[u64; orbistoun_core::GUEST_FLOAT_REGISTERS],
        ) -> u64 {
            0
        }

        install_float_handlers(vec![
            None,
            Some(answers as orbistoun_core::GuestFloatFn),
            None,
        ]);

        assert!(
            is_implemented(1),
            "a float handler is an implementation; asking only the integer table hid it"
        );
        // And the negative, so this cannot pass by reporting everything as implemented.
        assert!(!is_implemented(0), "an empty slot is still empty");
        assert!(!is_implemented(2), "and so is one past it");
        assert_eq!(
            implemented_count(),
            1,
            "the count reads both tables or it undercounts the same way"
        );
    }

    #[test]
    fn the_conforming_entry_alignment_is_eight_past_sixteen() {
        // System V requires `rsp % 16 == 0` immediately *before* a call; the call pushes
        // eight bytes of return address, so a callee begins eight past alignment. Every
        // compiler does its own arithmetic from that assumption before using an
        // instruction that moves sixteen bytes at once.
        assert!(super::entry_alignment_conforms(0x1008));
        assert!(super::entry_alignment_conforms(0x6000_0080_0d18));

        // A stack that is *fully* aligned at entry is just as wrong as one off by four:
        // it means whoever transferred control did not push a return address, which is a
        // `jmp` pretending to be a `call`.
        assert!(!super::entry_alignment_conforms(0x1000));
        assert!(!super::entry_alignment_conforms(0x1004));
    }

    #[test]
    fn only_misaligned_calls_are_recorded_and_the_first_one_is_kept() {
        // Both properties in one test because the counters are process-global and the
        // harness runs tests in parallel - separate tests would pollute each other, and a
        // flaky test about a diagnostic is worse than no test at all.
        //
        // Measured as deltas for the same reason: whatever else ran first is irrelevant.
        let before = super::abi_conformance();

        // Telemetry that fired on correct behaviour would bury the case it exists for.
        super::record_alignment(1, 1, 0x1008);
        assert_eq!(
            super::abi_conformance().misaligned_calls,
            before.misaligned_calls,
            "a conforming call must record nothing"
        );

        super::record_alignment(7, 3, 0x2000);
        super::record_alignment(9, 4, 0x3000);
        let after = super::abi_conformance();
        assert_eq!(after.misaligned_calls, before.misaligned_calls + 2);

        // Later misalignment is usually the first one's consequence, so the earliest is
        // the only one worth a slot.
        let kept = before.first_misaligned.or(Some((7, 3, 0x2000)));
        assert_eq!(after.first_misaligned, kept, "the earliest offender wins");
    }

    #[test]
    fn the_trampoline_has_a_real_address() {
        // A zero here would make every thunk jump to the null page, and the fault would
        // look like a guest bug rather than a build one.
        assert_ne!(trampoline_address(), 0);
    }

    #[test]
    fn six_argument_registers_are_saved() {
        // System V passes six integers in registers; the seventh onwards is on the
        // stack and deliberately not captured.
        assert_eq!(SAVED_ARGUMENT_REGISTERS, 6);
    }

    #[test]
    fn the_ring_is_bounded_so_the_call_path_never_allocates() {
        // Principle 9: a sink that allocates on a guest thread has changed the program
        // it observes.
        const { assert!(MAX_RECORDED_CALLS > 0) }
        assert!(MAX_RECORDED_CALLS.is_power_of_two());
    }
}
