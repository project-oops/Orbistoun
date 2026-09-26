//! Where every thunk lands, and what it records.
//!
//! One naked trampoline serves the whole table. Each thunk puts its index in `r10`, scratch under
//! System V and so the one register that carries a value in without destroying an argument. Rust
//! cannot read a register, so the trampoline spills the argument registers and re-presents them as
//! an ordinary call.
//!
//! System V requires `rsp % 16 == 0` before a `call`, so the trampoline arrives at `rsp % 16 == 8`;
//! `sub rsp, 8` restores alignment before the pushes. A mistake here only shows when a callee runs
//! an aligned SSE instruction against a stack slot, far from the cause.
//!
//! Recording takes no allocation and no lock (D018): counters are allocated when the table is built
//! and the call path does relaxed atomic adds. A bounded ring keeps calls in order, because the
//! sequence says what the guest was trying to do.

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use orbistoun_core::GuestError;

/// How many calls are kept in order before only counts are recorded.
///
/// Bounded because an unbounded log would allocate on the call path.
pub const MAX_RECORDED_CALLS: usize = 8192;

/// Number of argument registers System V passes in.
///
/// The same count the subsystem crates write implementations against, so the trampoline and the
/// functions it calls agree on how many it spilled.
pub const SAVED_ARGUMENT_REGISTERS: usize = orbistoun_core::GUEST_ARG_REGISTERS;

/// What `rsp % 16` must be when a callee is entered.
///
/// System V requires `rsp % 16 == 0` immediately before a `call`, and the call pushes an eight-byte
/// return address, so the callee begins eight past alignment. Compilers align the stack for 16-byte
/// instructions from that starting point.
pub const EXPECTED_ENTRY_REMAINDER: u64 = 8;

/// Whether a stack pointer at a callee's first instruction obeys the convention.
///
/// Pure, so the rule is testable without a guest; the code that uses it runs inside a naked
/// trampoline.
pub const fn entry_alignment_conforms(entry_rsp: u64) -> bool {
    entry_rsp % 16 == EXPECTED_ENTRY_REMAINDER
}

/// Calls that arrived on a stack the convention says is impossible.
static MISALIGNED_CALLS: AtomicU64 = AtomicU64::new(0);
/// Sequence number of the first such call, or `u64::MAX` if there has not been one.
static FIRST_MISALIGNED_SEQUENCE: AtomicU64 = AtomicU64::new(u64::MAX);
/// The `rsp` that first broke the rule, kept whole: the full value says which region the stack was
/// in.
static FIRST_MISALIGNED_RSP: AtomicU64 = AtomicU64::new(0);
/// Import index of the first offender.
static FIRST_MISALIGNED_INDEX: AtomicU64 = AtomicU64::new(0);

/// What the guest's calls looked like against the calling convention.
///
/// Telemetry rather than a check: nothing refuses a call or corrects a stack, because forcing
/// alignment would leave the guest misaligned internally and turn a loud fault into silent
/// corruption (D159).
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
/// System V passes the first six integer arguments in registers and the rest on the stack, above
/// the return address. A variadic call such as `printf` with a format and six numbers needs nine,
/// so the overflow area is published for the length of the call and the renderer reads it when the
/// registers run out: the `overflow_arg_area` of the psABI `va_list`, reached from the other side.
///
/// The count is still the format string's word; reading past what the guest passed gives whatever
/// the stack held, as a real `printf` would. Zero when no guest call is in progress, or when the
/// stack pointer was not one.
mod overflow {
    use std::cell::Cell;

    thread_local! {
        /// The first stack argument of the call this thread is inside.
        static AREA: Cell<u64> = const { Cell::new(0) };
    }

    /// Publishes the area for a call, answering what was there before.
    ///
    /// Saved and restored rather than cleared, so an implementation that calls back into another
    /// import does not leave the outer call reading nothing.
    pub(super) fn begin(entry_rsp: u64) -> u64 {
        // `[entry_rsp]` is the return address the guest's `call` pushed, so the first argument that
        // did not fit is the word above it.
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
/// A `call` pushes the return address and leaves `rsp` pointing at it, and the thunk reaches the
/// trampoline by a `jmp`, which pushes nothing, so that word is still the top of the stack. Zero
/// when the pointer is not word-aligned, since the word there is then not a return address.
fn call_site(entry_rsp: u64) -> u64 {
    if entry_rsp == 0 || entry_rsp % 8 != 0 {
        return 0;
    }
    let Ok(at) = usize::try_from(entry_rsp) else {
        return 0;
    };
    // SAFETY: the processor pushed a return address at exactly this location to reach the thunk
    // that jumped here, so the word is present and readable. Reading it does not disturb the
    // guest's stack.
    unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at)) }
}

/// Records one call's incoming stack alignment.
///
/// Two relaxed adds in the common case (D018). The "first" fields are written with a
/// compare-and-swap on the sequence so the earliest offender wins when threads race; later
/// misalignment is usually its consequence.
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
/// Circular: it holds the last [`MAX_RECORDED_CALLS`] calls, the neighbourhood of a wall (D571).
static RING: [AtomicU64; MAX_RECORDED_CALLS] = [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// Which call each slot holds, as `sequence + 1`, parallel to [`RING`].
///
/// Needed because the ring wraps: slot 3 might hold call 3 or call 8,195.
static RING_SEQ: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// How many of a run's first calls are kept whatever else happens.
///
/// The circular ring answers "what did it call last"; this answers "what did it call first" (D571).
/// Large enough to compare the calls around a branch that differs between runs of one title, and
/// cheap: a store per call below this count and a load afterwards.
pub const OPENING_CALLS: usize = 2048;

/// The opening of the run: `index + 1` for the first [`OPENING_CALLS`] calls, never overwritten.
static RING_OPENING: [AtomicU64; OPENING_CALLS] = [const { AtomicU64::new(0) }; OPENING_CALLS];

/// Where each opening call came from, parallel to [`RING_OPENING`].
///
/// A name says what ran; an address says which code ran it.
static RING_OPENING_FROM: [AtomicU64; OPENING_CALLS] = [const { AtomicU64::new(0) }; OPENING_CALLS];

/// The opening record must be the cheaper one, or keeping it separately buys nothing. A
/// compile-time assertion, since the compiler can settle it.
const _: () = assert!(OPENING_CALLS < MAX_RECORDED_CALLS);

/// Every integer argument of each recorded call, parallel to [`RING`], six per call.
///
/// A placeholder handed on as a size shows up only if every argument is recorded, not just the
/// first. The stores are bounded by the ring size; the static cost is the arrays.
static RING_ARGS: [AtomicU64; MAX_RECORDED_CALLS * SAVED_ARGUMENT_REGISTERS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS * SAVED_ARGUMENT_REGISTERS];

/// Which host thread made each call, as its own identifier.
///
/// Lets a reader tell which recent calls happened on the thread that faulted: the tail is every
/// thread's and a fault is one thread's (D621). The host thread rather than the guest handle,
/// because this crate is below the one that issues guest handles; the report joins the two.
static RING_THREAD: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// Where each recorded call came from, parallel to [`RING`].
///
/// A count says a title calls `memset` three hundred times; a call site says which three places,
/// turning a trace into a map of the guest's own code (D018). Free to capture: the return address
/// is the word at the stack pointer the trampoline already carries.
static RING_FROM: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// What each recorded call answered, parallel to [`RING`].
///
/// Shows an implemented function answering a wrong value the guest then trusts (D459). Written
/// after the handler returns, so [`RING_RETURNED`] says whether it was written: any `u64`, zero
/// included, is a legitimate answer, and a call still running must read as unknown.
static RING_RET: [AtomicU64; MAX_RECORDED_CALLS] =
    [const { AtomicU64::new(0) }; MAX_RECORDED_CALLS];

/// Whether [`RING_RET`] holds a real answer for a slot yet: `1` once written, `0` before.
///
/// Stored with `Release` after the answer and read with `Acquire`, so a reader that sees the flag
/// sees the answer that goes with it.
static RING_RETURNED: [AtomicU8; MAX_RECORDED_CALLS] =
    [const { AtomicU8::new(0) }; MAX_RECORDED_CALLS];

/// Per-import call counts. Allocated once when a table is built, never on the call path.
static COUNTS: std::sync::OnceLock<Box<[AtomicU64]>> = std::sync::OnceLock::new();

/// The shape bits an argument value ORs into its slot's profile, so the kinds a slot saw across
/// calls accumulate into a picture of what that argument is.
///
/// The firmware behind an import is opaque, but the guest's own calls describe it: a slot always
/// pointing into guest memory is a pointer, one always small is a scalar or size, one only ever
/// zero is unused, and one sometimes a pointer and sometimes zero is an optional pointer. This is a
/// lower bound inferred from behaviour, never a proof.
pub const SHAPE_ZERO: u8 = 0x1;
/// A small value: a flag, a count, a size, not an address. See [`SHAPE_ZERO`].
pub const SHAPE_SCALAR: u8 = 0x2;
/// A value inside the guest's address space: a pointer. See [`SHAPE_ZERO`].
pub const SHAPE_POINTER: u8 = 0x4;
/// Anything else: a large non-address value. See [`SHAPE_ZERO`].
pub const SHAPE_OTHER: u8 = 0x8;

/// Classifies one argument value into its [shape bit](SHAPE_ZERO).
///
/// Pure, so the boundaries are tested without a running guest. The guest address space runs from
/// the image base up through the stack and the direct-memory pool (`0x4000...`..`0x8000...`); below
/// a page a value is a scalar.
#[must_use]
pub const fn classify_arg(v: u64) -> u8 {
    /// The bottom of the guest's address space (image base), above every scalar a guest passes.
    const GUEST_LOW: u64 = 0x4000_0000_0000;
    /// One past the top of it.
    const GUEST_HIGH: u64 = 0x8000_0000_0000;
    /// Under a page: a flag, a small count or a size, never an address.
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
/// A register that has ever held a pointer is a pointer slot, so categories are read widest first
/// (pointer, large non-address, scalar); `?` marks a slot that was also zero on some call. A slot
/// only ever zero is an argument always passed as nought: counted in the arity, carrying no other
/// evidence.
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
/// The arity counts up to the last slot that ever carried anything; trailing untouched registers
/// are arguments the guest did not pass. An import called with no arguments, or never sampled,
/// renders `()` (see [`classify_arg`]).
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
/// Bounded so the busiest imports pay the extra stores only at the start; the shape is settled in
/// far fewer calls.
const SHAPE_SAMPLE_LIMIT: u64 = 16;

/// Per-import argument shapes: [`SAVED_ARGUMENT_REGISTERS`] slots per import, each an OR of the
/// [shape bits](SHAPE_ZERO) its calls carried. Allocated with [`COUNTS`], never on the call path.
static SHAPES: std::sync::OnceLock<Box<[AtomicU8]>> = std::sync::OnceLock::new();

pub use orbistoun_core::GuestFn;

/// How many imports the guest may call before the run is stopped.
///
/// A wall-clock limit fixes the duration and lets the call count vary, and verdicts are read off
/// the call count. A budget fixes the count, so two runs of one build stop at the same call. It
/// does not replace the wall-clock limit: a guest idling without calls never reaches a budget
/// (D238).
///
/// Starts at `u64::MAX`, so the ordinary path is one relaxed load and a comparison that is always
/// false.
static CALL_BUDGET: AtomicU64 = AtomicU64::new(u64::MAX);

/// What to do when the budget is reached. Installed by the worker, which owns the trace.
///
/// A callback, because collecting, persisting and summarising a trace live above this crate.
static ON_BUDGET: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Stops the run after `budget` import calls, by calling `on_exceeded`.
pub fn install_call_budget(budget: u64, on_exceeded: fn()) {
    let _ = ON_BUDGET.set(on_exceeded);
    CALL_BUDGET.store(budget, Ordering::Relaxed);
}

/// Implementations that speak in floating-point registers, by symbol index.
///
/// Separate from [`HANDLERS`] so integer implementations do not carry an unused array on every call
/// (D268).
static FLOAT_HANDLERS: std::sync::OnceLock<Box<[Option<orbistoun_core::GuestFloatFn>]>> =
    std::sync::OnceLock::new();

/// Records the floating-point implementations for a table of `count` entries.
pub fn install_float_handlers(handlers: Vec<Option<orbistoun_core::GuestFloatFn>>) {
    let _ = FLOAT_HANDLERS.set(handlers.into_boxed_slice());
}

/// Implementations by symbol index, or `None` where there is none.
///
/// Consulted at the point the guest calls, so an implementation replaces the recording stub (D082).
static HANDLERS: std::sync::OnceLock<Box<[Option<GuestFn>]>> = std::sync::OnceLock::new();

/// What an unimplemented stub hands back, by symbol index.
///
/// `None` means the ordinary error code. A pointer- or handle-returning function answers zero,
/// because the caller uses the answer as data, and an error code in a pointer register is a wild
/// pointer (D125).
static STUB_RETURNS: std::sync::OnceLock<Box<[Option<u64>]>> = std::sync::OnceLock::new();

/// Records what each unimplemented stub should answer.
pub fn install_stub_returns(values: Vec<Option<u64>>) {
    let _ = STUB_RETURNS.set(values.into_boxed_slice());
}

/// A forced answer per symbol index, overriding both the policy and the error code.
///
/// Separate from [`STUB_RETURNS`], which the service installs from the policy file as a `OnceLock`:
/// a second install there would be a silent no-op. A distinct layer, consulted first, cannot lose
/// that race (D166).
static FORCED_RETURNS: std::sync::OnceLock<Box<[Option<u64>]>> = std::sync::OnceLock::new();

/// How many calls were answered with a forced value.
static RETURNS_FORCED: AtomicU64 = AtomicU64::new(0);

/// Makes named imports answer a chosen value for one run.
///
/// A diagnostic rather than a `StubPolicy` entry, because the policy is keyed by symbol name and
/// carries a 32-bit code: it cannot address an unnamed function or express a 64-bit region base.
pub fn install_forced_returns(values: Vec<Option<u64>>) {
    let _ = FORCED_RETURNS.set(values.into_boxed_slice());
}

/// The answer forced for `index`, counting it when there is one.
///
/// Consulted for implemented and unimplemented imports alike, so an implemented function's answer
/// can be tested too (D166).
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
/// Asked when a run is summarised, not on the call path: "called and not implemented" is the most
/// directly actionable thing a run reports (D179).
pub fn is_implemented(index: usize) -> bool {
    attached(&HANDLERS, index) || attached(&FLOAT_HANDLERS, index)
}

/// Whether one table has a handler in a slot.
///
/// Both tables are asked by callers: a function answering in `xmm0` is as implemented as one
/// answering in `rax` (D268).
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
/// The table carries stubs past the guest's own imports for names it may resolve at run time
/// (D366), each with a handler by construction. A report about the guest's imports passes the
/// guest's import count here so those are not counted.
pub fn implemented_count_within(limit: usize) -> usize {
    // The two tables are disjoint by construction, so this is a sum rather than a union.
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
/// Small: a dump is taken for an import's first few calls only, and a fixed ceiling keeps the call
/// path allocation-free (D194).
pub const MAX_DUMPS: usize = 512;

/// How many calls of one import are worth dumping.
///
/// Two, so a value that changes between calls can be told from one that does not: an out-parameter
/// from a constant.
const DUMPS_PER_IMPORT: u32 = 2;

/// Bytes captured from each argument that points somewhere readable.
///
/// Enough for a small struct or the start of a string; a bigger structure still shows its first
/// fields, which identify it (a size, a version, a magic).
pub const DUMP_BYTES: usize = 32;

/// Words per dump: the words of the capture itself.
const DUMP_WORDS: usize = DUMP_BYTES / 8;

/// Where guest memory is known to be readable, as (base, len) pairs.
///
/// The safety precondition and the filter in one: dereferencing a small integer or length would
/// fault inside the emulator, so only addresses inside something this process mapped are read.
/// Installed by the layer that does the mapping, because this crate must not depend on it.
static READABLE: std::sync::OnceLock<Box<[(u64, u64)]>> = std::sync::OnceLock::new();

/// Records where guest memory may safely be read from.
pub fn install_readable_ranges(ranges: Vec<(u64, u64)>) {
    let _ = READABLE.set(ranges.into_boxed_slice());
}

/// How many ranges a run can add after the first are published.
///
/// Fixed because this is read from the guest's own stack, where allocating is not allowed (D381).
/// Sized for guest thread stacks plus guest mappings; a range past the limit is counted in
/// [`DROPPED_RANGES`].
const MOST_EXTRA_RANGES: usize = 512;

/// Ranges published after the run started, as `(base, len)` pairs.
///
/// A zero length marks an empty slot: base zero is a legitimate address and length zero is not a
/// range.
static EXTRA_RANGES: [(AtomicU64, AtomicU64); MOST_EXTRA_RANGES] =
    [const { (AtomicU64::new(0), AtomicU64::new(0)) }; MOST_EXTRA_RANGES];

/// How many extra ranges have been published.
static EXTRA_COUNT: AtomicU64 = AtomicU64::new(0);

/// How many ranges were published after the table was full.
static DROPPED_RANGES: AtomicU64 = AtomicU64::new(0);

/// How many readable ranges this run could not remember.
///
/// Non-zero means the argument dump has a blind spot of known size, which otherwise prints like a
/// wrong pointer.
#[must_use]
pub fn dropped_ranges() -> u64 {
    DROPPED_RANGES.load(Ordering::Relaxed)
}

/// Publishes a span of guest memory that appeared after the run started.
///
/// The first list, published before the guest is entered, holds the image and the main stack. A
/// guest thread's stack does not exist yet at that moment, and without this every argument on it
/// would read as an unmapped address rather than a region the tool cannot see (D387).
/// Allocation-free and lock-free, because a dump runs on the guest's stack.
pub fn note_readable_range(base: u64, len: u64) {
    if len == 0 {
        return;
    }
    let slot = EXTRA_COUNT.fetch_add(1, Ordering::Relaxed);
    let Ok(slot) = usize::try_from(slot) else {
        return;
    };
    let Some((held_base, held_len)) = EXTRA_RANGES.get(slot) else {
        // Counted, because a pointer into a dropped range otherwise reports like a wild pointer.
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
/// A count and an address pointing at nothing this run mapped would otherwise render identically as
/// a bare number, and the second is a finding while the first is ordinary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointing {
    /// Below everything this process mapped for the guest, so not an address: a size, a flag, a
    /// count. Evidence in its own right.
    Scalar,
    /// Inside a mapped region, and the bytes were read.
    Mapped,
    /// Address-shaped, and no mapped region holds [`DUMP_BYTES`] readable bytes there.
    ///
    /// Either the address is wrong, or the run did not declare the region it points into; neither
    /// is a count.
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

    /// Back from the stored form. An unknown code reads as a scalar, the claim that asserts least.
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
/// The floor is the lowest base among the declared ranges rather than a second constant for where
/// guest memory starts.
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
/// The check the dumper makes before dereferencing, for everything that follows a guest pointer. A
/// pointer the guest computed may fault where it would have on hardware, but one it never set (a
/// `%s` read from an overflow area holding no arguments) is arbitrary stack contents and would
/// fault inside the renderer. One byte, because a string is followed a byte at a time.
#[must_use]
pub fn is_mapped(address: u64) -> bool {
    let published = READABLE.get().is_some_and(|ranges| {
        ranges
            .iter()
            .any(|&(base, len)| address >= base && address < base.saturating_add(len))
    });
    // Or a span that appeared after the run started, such as a guest thread's stack (D387).
    published || in_extra_range(address)
}

/// Whether ranges have been published at all.
///
/// A caller outside a run, such as a unit test, has none, and then no pointer is refused because
/// there is nothing to check against.
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
/// The one answer to "may I dereference this?": the dump asks it about its fixed window, and a
/// watch or snapshot about a typed length.
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
    // The whole window has to be inside one range, published or added later, because all of it is
    // read (D387).
    published || in_extra_range(address) && in_extra_range(end.saturating_sub(1))
}

/// Imports to dump even though something implements them, by symbol index.
///
/// Dumps otherwise fire only for unimplemented imports, and a suspect implementation needs its
/// arguments shown too. Opt-in by name, because the busiest imports are implemented and dumping
/// every implemented call would put an atomic increment on that path.
static FORCED: std::sync::OnceLock<Box<[bool]>> = std::sync::OnceLock::new();

/// Records which imports to dump regardless of whether they are implemented.
pub fn install_forced_dumps(forced: Vec<bool>) {
    let _ = FORCED.set(forced.into_boxed_slice());
}

/// Whether any import was named for a forced dump.
///
/// When one was, the default set is suppressed: a caller who named an import is asking about it,
/// and the buffer is small enough that the two compete.
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
/// A guest calling many unimplemented functions fills [`MAX_DUMPS`] early, so the report says the
/// tool ran out of room and by how much.
static DUMPS_DROPPED: AtomicU64 = AtomicU64::new(0);
/// Which import each dump belongs to, plus one so zero means empty.
static DUMP_IMPORT: [AtomicU64; MAX_DUMPS] = [const { AtomicU64::new(0) }; MAX_DUMPS];
/// Which argument position was dumped.
static DUMP_SLOT: [AtomicU64; MAX_DUMPS] = [const { AtomicU64::new(0) }; MAX_DUMPS];
/// The address the bytes came from.
static DUMP_ADDRESS: [AtomicU64; MAX_DUMPS] = [const { AtomicU64::new(0) }; MAX_DUMPS];
/// What the argument turned out to be, so a count and an address pointing at nothing are not both
/// shown as an empty buffer.
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
    /// Which import was called: an index into the table.
    pub index: u32,
    /// Which argument, counting from zero.
    pub slot: u8,
    /// The address the bytes were read from, which is also the argument's raw value.
    pub address: u64,
    /// What the argument turned out to be.
    ///
    /// A scalar argument is evidence too: a size, flag or count is recorded, not only pointers into
    /// mapped memory. See [`Pointing`].
    pub pointing: Pointing,
    /// The bytes, as they were at the moment of the call.
    pub bytes: [u8; DUMP_BYTES],
}

/// Where a forced write is allowed to land.
///
/// Not [`READABLE`]: that list includes the image, whose pages are protected after relocation, so a
/// write there would fault inside the emulator. Only the stack is installed here, where an
/// out-parameter lives.
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
/// A list with offsets, so one run can plant a different recognisable value in every candidate slot
/// of a structure, and whichever the guest uses names itself in what happens next.
pub type ForcedWrite = Box<[Plant]>;

/// What to plant for each import, by symbol index.
static FORCED_WRITES: std::sync::OnceLock<Box<[ForcedWrite]>> = std::sync::OnceLock::new();

/// How many forced writes actually landed.
static WRITES_DONE: AtomicU64 = AtomicU64::new(0);
/// Forced writes that could not be performed because the target was not writable.
static WRITES_REFUSED: AtomicU64 = AtomicU64::new(0);

/// What the policy says a stub writes, by symbol index.
///
/// The same operation as a forced write with a different life: a forced write is a diagnostic from
/// the environment, reported in the run's conditions; this is the known answer, data in a file in
/// force on every run. The value is a region base the service reserved before the guest started,
/// because reserving address space does not belong on the guest's stack (D300).
static POLICY_WRITES: std::sync::OnceLock<Box<[ForcedWrite]>> = std::sync::OnceLock::new();

/// Records what each stub writes, from the policy.
pub fn install_policy_writes(writes: Vec<ForcedWrite>) {
    let _ = POLICY_WRITES.set(writes.into_boxed_slice());
}

/// Records a region base each stub answers with, from the policy.
///
/// The same table a policy answer uses, since this is a value the function hands back; only its
/// origin differs (D300). Folded into [`install_stub_returns`]' table so there is one place the
/// answer is consulted.
pub fn install_policy_returns(returns: Vec<(usize, u64)>) {
    let Some(existing) = STUB_RETURNS.get() else {
        // Nothing has installed the policy answers yet, so this is the whole table.
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
    // A region wins over a scalar answer: `ok` is what a caller tests, a region is what it uses,
    // and the region is the more specific claim.
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
/// A `OnceLock` cannot be set twice, and region answers are resolved after the scalar ones, so they
/// land here and are consulted first rather than being lost to a second install.
static REPLACED_RETURNS: std::sync::OnceLock<Box<[Option<u64>]>> = std::sync::OnceLock::new();

/// How many answers the region table replaced.
static OVERRIDDEN_RETURNS: AtomicU64 = AtomicU64::new(0);

/// Records what each stub writes before it answers, from the environment.
///
/// A diagnostic, driven from the environment because the question is asked once, and reported in
/// the run's conditions so a verdict taken under it is never compared with an ordinary one. A known
/// answer belongs in the policy, through [`install_policy_writes`].
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
/// Plants a recognisable value so a run can show whether an argument is an out-parameter the guest
/// expects filled: the fault address follows it. Refusals are counted, because a diagnostic that
/// silently did nothing looks like one that ran and changed nothing.
fn forced_write(index: u64, args: *const u64) {
    apply_writes(&FORCED_WRITES, index, args);
    // The policy's writes, in the same pass and by the same code: only their origin and lifetime
    // differ from a forced write.
    apply_writes(&POLICY_WRITES, index, args);
}

/// Performs one table's writes for `index`.
///
/// Refusals are counted, because a write that silently did nothing looks like one that ran and
/// changed nothing.
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
        // Wrapped rather than saturated: a saturated address would be one nobody asked about, which
        // `is_writable` would then refuse.
        let target = pointer.wrapping_add(plant.offset as u64);
        if !is_writable(target) {
            WRITES_REFUSED.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let Ok(at) = usize::try_from(target) else {
            WRITES_REFUSED.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        // SAFETY: `is_writable` established that eight bytes from `target` lie inside a range this
        // process mapped read-write, so the store is in bounds and cannot fault.
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
/// At call time because the contents do not survive: a stack frame is reused within microseconds,
/// and reading it at summary time would give a precisely wrong answer. Bounded on every axis
/// (imports, calls, arguments, total dumps) and allocation-free (D194).
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
            // Counted, not merely refused: a dump nobody took otherwise looks like a call that
            // passed no arguments worth showing.
            DUMPS_DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let from: &[u8] = if readable {
            // SAFETY: this branch is taken only when `is_readable` established that `DUMP_BYTES`
            // from `value` lie inside a range this process mapped, so the whole span is readable.
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
/// The most recent, not the first: argument dumps keep an import's first calls, but the call
/// that went wrong is usually among the last, often after many good ones.
const CALLER_STACKS: usize = 4;

/// Stack words kept per call, from the return address up.
///
/// Four kibibytes: an allocator's own frames can take half a kibibyte, and the caller of interest
/// sits above them. The report picks out return addresses; this layer only copies.
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
/// An observation, not a walk: no frame pointer is followed, and the report marks which words are
/// addresses in guest code. A stale word from an older frame can look like a return address. Only a
/// span [`readable_span`] vouches for is read, so this cannot fault; allocation-free, because it
/// runs on the guest's stack (D381).
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
        // SAFETY: `readable_span` established that `bytes` from `entry_rsp` lie inside a range this
        // process mapped, and `at` is within that span, so the unaligned read of one word is in
        // bounds and cannot fault.
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
    /// Which import was called: an index into the table.
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
    /// Which import was called: an index into the table this thunk belongs to.
    pub index: u32,
    /// The call's integer arguments, in register order: `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.
    pub args: [u64; SAVED_ARGUMENT_REGISTERS],
    /// Which host thread made it, or zero when nothing recorded one. Only compared for equality.
    pub thread: u64,
    /// The guest address this call returns to, one instruction past the call site.
    ///
    /// Zero when it could not be read. Matches the addresses a fault's frame walk reports, so a
    /// stack trace and a call trace can be read against each other.
    pub from: u64,
    /// What this call answered in `rax`, or [`None`] if it had not returned when the record was
    /// read. `None` is not zero: zero is a real answer (`OK`).
    pub ret: Option<u64>,
}

/// Prepares per-import counters and argument-shape slots for a table of `count` entries.
///
/// Called once, at table construction, so the call path stays allocation-free.
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

/// The [shape bits](SHAPE_ZERO) inferred for each import's arguments, by index: one `[u8;
/// SAVED_ARGUMENT_REGISTERS]` per import, parallel to [`call_counts`].
///
/// Each slot is the OR of the categories that argument carried across the first
/// `SHAPE_SAMPLE_LIMIT` calls; `0` means the slot was never written, because the import was never
/// called or takes fewer arguments.
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
/// Reads the flag with `Acquire` against the recording store's `Release`, so a seen flag guarantees
/// the answer beside it belongs to it.
fn recorded_return(slot: usize) -> Option<u64> {
    (RING_RETURNED[slot].load(Ordering::Acquire) == 1)
        .then(|| RING_RET[slot].load(Ordering::Relaxed))
}

/// The call the guest most recently entered, if any.
///
/// Allocation-free, because the caller is a fault handler on a thread that has just faulted. It
/// reads the ring, so it cannot disagree with the trace; a call still being recorded reads as the
/// one before it, which had definitely started.
pub fn last_call() -> Option<RecordedCall> {
    // The highest sequence, not the highest slot: in a wrapped ring the newest call can sit
    // anywhere (D571).
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

/// Which import this thread is currently inside, if any.
///
/// [`last_call`] answers with the newest call any thread made, right for a fault handler and wrong
/// for an implementation asking what it is inside, such as the mapping record or the format trace
/// (D621). A thread that never entered a call answers `None`. Calls nest through callbacks, so the
/// previous value is restored on the way out; the cost is one thread-local `u32` written twice per
/// call.
pub fn current_call() -> Option<u32> {
    INSIDE.with(|inside| inside.get().checked_sub(1))
}

/// This host thread's own identifier, as the recorded calls carry it.
///
/// Lets a fault be paired with the calls made on the same thread: the fault handler runs on the
/// faulting thread, so it reads its own (D621).
#[must_use]
pub fn current_thread() -> u64 {
    host_thread()
}

/// A stable identifier for the calling host thread.
///
/// The address of a thread-local marker: unique per thread, constant within one, never zero, and a
/// load rather than a system call. Public so the thread registry can record which host thread a
/// guest handle runs on, joining the recorded calls to the thread table.
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
    /// Thread-local, because a guest thread has no small dense identifier here and recording must
    /// not allocate.
    static INSIDE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
/// The most recent calls, in the order the guest made them.
///
/// At most [`MAX_RECORDED_CALLS`]; [`total_calls`] says how many there were. Ordered by the
/// recorded sequence, because in a wrapped ring slot order says nothing about the guest (D571).
pub fn recorded_calls() -> Vec<RecordedCall> {
    let mut out: Vec<RecordedCall> = (0..MAX_RECORDED_CALLS)
        .filter_map(|i| {
            let stored = RING[i].load(Ordering::Relaxed);
            // Zero means the slot was claimed but not yet written by another thread. Skipped, since
            // import zero is a real index.
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
/// Kept apart from the circular ring; the halt summary quotes these (D571).
pub fn opening_calls() -> Vec<u32> {
    RING_OPENING
        .iter()
        .filter_map(|slot| slot.load(Ordering::Relaxed).checked_sub(1))
        .map(|index| index as u32)
        .collect()
}

/// The run's first calls with the address each was made from, in order.
///
/// The position is the sequence number, so the record can be diffed against the mapping record,
/// whose entries are indexed by call ordinal. Stops at the first empty slot rather than filtering,
/// because a gap would shift every later call's number.
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
/// `args` points at the six argument registers spilled by [`trampoline`], in System V order; the
/// seventh argument onwards is on the guest stack and not captured here. `entry_rsp` is the stack
/// pointer as the guest's `call` left it, the one number that says whether the guest obeys the
/// calling convention (D159).
///
/// # Safety
///
/// `args` must point to [`SAVED_ARGUMENT_REGISTERS`] readable `u64` values. The trampoline is the
/// only caller and satisfies this by construction.
unsafe extern "sysv64" fn on_guest_call(
    index: u64,
    args: *const u64,
    entry_rsp: u64,
    floats: *mut u64,
) -> u64 {
    // A backgrounded title parks here: every guest call passes through, and a thread stopped here
    // is in our code holding no guest lock, unlike one frozen at an arbitrary instruction that may
    // hold the host heap lock (D344). Before the sequence number, so a parked call is not yet
    // counted.
    orbistoun_core::park::check();

    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);

    // Checked first, so the run stops at exactly the budgeted call rather than near it, as a
    // polling watcher thread would (D238).
    if sequence >= CALL_BUDGET.load(Ordering::Relaxed) {
        if let Some(stop) = ON_BUDGET.get() {
            stop();
        }
    }

    record_alignment(sequence, index, entry_rsp);

    // SAFETY: `args` is this function's own parameter, forwarded unchanged, so it still points at
    // the `SAVED_ARGUMENT_REGISTERS` values the trampoline spilled.
    unsafe { sample_argument_shapes(index, args) };

    // SAFETY: as above; `args` is forwarded unchanged.
    unsafe { record_call(sequence, index, args, entry_rsp) };

    // Looked up once, and the dump decided from the result, so implemented calls pay one table
    // lookup.
    let handler = HANDLERS
        .get()
        .and_then(|h| h.get(index as usize))
        .and_then(|h| *h);

    // Only for calls nothing implements, which keeps dumping off the hot path (D194). A forced list
    // narrows the dump to itself: somebody who names an import is asking about that import.
    if is_forced(index as usize) || (handler.is_none() && !anything_forced()) {
        dump_arguments(index, args);
    }
    if is_forced(index as usize) {
        capture_caller_stack(sequence, index, entry_rsp);
    }

    // After the dump, so the dump records what the guest passed rather than what this planted.
    forced_write(index, args);

    // The dispatch is split out so every answer leaves through one point, where the return below is
    // recorded; `handler` is handed over rather than looked up again. `INSIDE` brackets exactly the
    // handler, so an implementation asking what it is inside gets its own thread's answer (D621).
    let outer = INSIDE.with(|inside| inside.replace((index as u32).wrapping_add(1)));
    // SAFETY: `args`, `floats` and `entry_rsp` are this function's own parameters, forwarded
    // unchanged, so they still satisfy the contract the trampoline established.
    let answer = unsafe { resolve(index, args, entry_rsp, handler, floats) };
    INSIDE.with(|inside| inside.set(outer));

    record_return(sequence, answer);

    answer
}

/// Counts one call of `index` and, for its first calls, folds its argument shapes in.
///
/// # Safety
///
/// As [`on_guest_call`]: `args` points to [`SAVED_ARGUMENT_REGISTERS`] readable `u64` values.
#[inline]
unsafe fn sample_argument_shapes(index: u64, args: *const u64) {
    if let Some(counts) = COUNTS.get() {
        if let Some(counter) = counts.get(index as usize) {
            // `fetch_add` answers the count before this call, so the first `SHAPE_SAMPLE_LIMIT`
            // calls of each import contribute their shapes and later calls pay only the increment.
            let seen = counter.fetch_add(1, Ordering::Relaxed);
            if seen < SHAPE_SAMPLE_LIMIT {
                if let Some(shapes) = SHAPES.get() {
                    let base = index as usize * SAVED_ARGUMENT_REGISTERS;
                    for register in 0..SAVED_ARGUMENT_REGISTERS {
                        // SAFETY: the caller guarantees `SAVED_ARGUMENT_REGISTERS` readable values
                        // in register order, and `register` is bounded by that count, so the offset
                        // stays inside the caller's array.
                        let at = unsafe { args.add(register) };
                        // SAFETY: `at` is inside the caller's array, per the block above, and these
                        // are plain integers with no alignment or validity demand beyond being
                        // readable.
                        let value = unsafe { at.read() };
                        if let Some(slot) = shapes.get(base + register) {
                            slot.fetch_or(classify_arg(value), Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }
}

/// Writes one call into the opening record and the circular ring.
///
/// # Safety
///
/// As [`on_guest_call`]: `args` points to [`SAVED_ARGUMENT_REGISTERS`] readable `u64` values.
#[inline]
unsafe fn record_call(sequence: u64, index: u64, args: *const u64, entry_rsp: u64) {
    if let Ok(position) = usize::try_from(sequence) {
        // The opening, kept whole and never overwritten (D571).
        if let Some(opening) = RING_OPENING.get(position) {
            // The call site first, so a reader that sees a populated index never finds a stale
            // address beside it; the main ring uses the same ordering.
            if let Some(from) = RING_OPENING_FROM.get(position) {
                from.store(call_site(entry_rsp), Ordering::Relaxed);
            }
            opening.store(index + 1, Ordering::Relaxed);
        }
        {
            let slot = position % MAX_RECORDED_CALLS;
            for register in 0..SAVED_ARGUMENT_REGISTERS {
                // SAFETY: the caller guarantees `SAVED_ARGUMENT_REGISTERS` readable values in
                // register order, and `register` is bounded by that count, so the offset stays
                // inside the caller's array.
                let at = unsafe { args.add(register) };
                // SAFETY: `at` is inside the caller's array, per the block above, and these are
                // plain integers with no alignment or validity demand beyond being readable.
                let value = unsafe { at.read() };
                RING_ARGS[slot * SAVED_ARGUMENT_REGISTERS + register]
                    .store(value, Ordering::Relaxed);
            }
            RING_FROM[slot].store(call_site(entry_rsp), Ordering::Relaxed);
            RING_THREAD[slot].store(host_thread(), Ordering::Relaxed);
            // The previous occupant's answer is retired before this call claims the slot, so a call
            // still running in a recycled slot (a blocking wait, say) reads as unknown rather than
            // reporting the last occupant's answer. Ordered before the sequence with `Release`, so
            // a reader that sees this call's number never sees the last call's flag.
            RING_RETURNED[slot].store(0, Ordering::Release);
            RING_SEQ[slot].store(sequence.wrapping_add(1), Ordering::Relaxed);
            // Written after the argument and the sequence, so a reader never sees a populated index
            // pointing at a stale argument or the wrong call's number.
            RING[slot].store(index.wrapping_add(1), Ordering::Relaxed);
        }
    }
}

/// Records a call's answer in its ring slot, if the slot is still that call's.
#[inline]
fn record_return(sequence: u64, answer: u64) {
    // Written after the handler ran, so a slot read before then reads as unknown rather than as the
    // initial zero, which `OK` also is (D459).
    if let Ok(position) = usize::try_from(sequence) {
        // The same slot the call went into, and only if it is still that call's: after a wrap a
        // later call owns the slot, and writing here would attribute the answer to the wrong
        // function (D571).
        let slot = position % MAX_RECORDED_CALLS;
        if RING_SEQ[slot].load(Ordering::Relaxed) == sequence.wrapping_add(1) {
            RING_RET[slot].store(answer, Ordering::Relaxed);
            // Release, paired with the Acquire in `recorded_return`, so seeing the flag set
            // guarantees the answer beside it is this call's.
            RING_RETURNED[slot].store(1, Ordering::Release);
        }
    }
}

/// Dispatches one already-recorded call to whatever answers it and returns what goes back to the
/// guest in `rax` (and, for a float function, `xmm0` via `floats`).
///
/// Split out of [`on_guest_call`] so every answer leaves through one point, where the caller
/// records the return. `handler` is passed in because the caller already looked it up.
///
/// # Safety
///
/// Same contract as [`on_guest_call`]: `args` points at [`SAVED_ARGUMENT_REGISTERS`] readable `u64`
/// values and `floats` at [`orbistoun_core::GUEST_FLOAT_REGISTERS`] writable ones, both outliving
/// the call.
unsafe fn resolve(
    index: u64,
    args: *const u64,
    entry_rsp: u64,
    handler: Option<GuestFn>,
    floats: *mut u64,
) -> u64 {
    // Before the integer handler: a function that answers in `xmm0` has nothing useful in `rax`,
    // and the two tables are disjoint (D268).
    if let Some(float_handler) = FLOAT_HANDLERS
        .get()
        .and_then(|h| h.get(index as usize))
        .and_then(|h| *h)
    {
        // SAFETY: the trampoline spilled six integer argument registers to the stack immediately
        // below this frame, and the array outlives the call.
        let ints = unsafe { &*args.cast::<[u64; SAVED_ARGUMENT_REGISTERS]>() };
        // SAFETY: the same spill put eight floating-point argument registers below those.
        let float_args = unsafe { &*floats.cast::<[u64; orbistoun_core::GUEST_FLOAT_REGISTERS]>() };
        let previous = overflow::begin(entry_rsp);
        let answer = float_handler(ints, float_args);
        overflow::end(previous);
        // Written where the trampoline loads `xmm0` from.
        // SAFETY: the same eight-slot array, which is writable and this thread's own.
        unsafe { floats.write(answer) };
        // `rax` too, so a function whose result is read as an integer is not handed a stale one.
        return answer;
    }

    if let Some(handler) = handler {
        // SAFETY: the caller guarantees six readable values, which is the array this reborrows. The
        // trampoline spilled them and they outlive this call.
        let args: &[u64; SAVED_ARGUMENT_REGISTERS] = unsafe { &*args.cast() };
        // Published for the length of the call, so a variadic implementation can read the arguments
        // that did not fit in registers.
        let previous = overflow::begin(entry_rsp);
        let answer = handler(args);
        overflow::end(previous);
        // The implementation still runs; only its answer is replaced, so the rest of the program
        // behaves as it did (D166). The `get()` is one atomic load that is `None` on an ordinary
        // run.
        if let Some(value) = forced_answer(index) {
            return value;
        }
        return answer;
    }

    // Forced answers first, so a diagnostic reaches a function whose answer the policy already
    // sets, and counted, so a forced return that matched nothing is visible (D166). Then
    // pointer-returning functions answer zero, since an error code would be a wild pointer.
    if let Some(value) = forced_answer(index) {
        return value;
    }

    // Regions first: they are resolved after the scalar answers and cannot overwrite a `OnceLock`,
    // so they live in their own table and win where both have an entry (D300).
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

    // Otherwise never zero, which a guest would read as success: an explicit "not handled" costs
    // the same as a wrong answer. Widened rather than truncated, so a caller reading the full
    // register never sees a stale upper half.
    u64::from(GuestError::Unimplemented.as_raw())
}

/// The address every thunk jumps to.
pub fn trampoline_address() -> u64 {
    // Named with its full type before casting: a bare `as usize` on a function item is a different,
    // easier-to-get-wrong conversion.
    let f: unsafe extern "sysv64" fn() = trampoline;
    f as usize as u64
}

/// Hand-written entry point shared by every thunk.
///
/// Spills the argument registers to the stack, presents them as an ordinary call, and passes the
/// handler's return value back to the guest in `rax`. `sub rsp, 8` corrects the alignment a `call`
/// left odd; the six pushes move 48 bytes, which preserves it. Naked because a compiler-inserted
/// prologue would clobber the argument registers before they are saved.
#[unsafe(naked)]
unsafe extern "sysv64" fn trampoline() {
    core::arch::naked_asm!(
        // Before anything else: what the guest's `call` left behind. `r11` is scratch under System
        // V and already dead here (the thunk jumped through it), so it carries a value across the
        // spill without destroying an argument (D159).
        "mov r11, rsp",
        "sub rsp, 8",
        "push r9",
        "push r8",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        // The eight floating-point argument registers, low halves: a `double`, or a `float` in its
        // low half. Spilled unconditionally, because a maths function's argument arrives only here
        // (D268). A cheaper integer path would be a second trampoline chosen per import at table
        // build time.
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
        // The integer array sits above the floating-point one.
        "lea rsi, [rsp + 64]",
        // Third argument: the incoming stack pointer. `rdx` is safe to clobber here: its guest
        // value was spilled by the push above.
        "mov rdx, r11",
        // Fourth: the floating-point array, which the handler also writes its answer into.
        "mov rcx, rsp",
        "call {handler}",
        // Whatever the handler left in the first slot becomes `xmm0`. For an integer function that
        // is the value the guest passed in, written straight back, so nothing is disturbed.
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

    /// A slot's sequence is stored offset by one, so an untouched slot is empty rather than call
    /// zero.
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

    /// Each argument value lands in exactly the category its magnitude names, at every boundary.
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

    /// A slot that is sometimes a pointer and sometimes zero reads as an optional pointer.
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

    /// The rendered signature reports arity and marks nullable slots.
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
    /// One test rather than several: the tables are process-global and set once, so a second test
    /// installing its own would race this one.
    #[test]
    fn a_function_answering_in_a_float_register_is_implemented() {
        /// Stands in for a maths function; what matters is that it is attached.
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
        // A callee begins eight past alignment: the call pushes eight bytes onto a 16-aligned
        // stack.
        assert!(super::entry_alignment_conforms(0x1008));
        assert!(super::entry_alignment_conforms(0x6000_0080_0d18));

        // A fully aligned stack at entry is as wrong as one off by four: it means a `jmp` posed as
        // a `call` and pushed no return address.
        assert!(!super::entry_alignment_conforms(0x1000));
        assert!(!super::entry_alignment_conforms(0x1004));
    }

    #[test]
    fn only_misaligned_calls_are_recorded_and_the_first_one_is_kept() {
        // Both properties in one test because the counters are process-global and tests run in
        // parallel. Measured as deltas for the same reason.
        let before = super::abi_conformance();

        // Correct behaviour does not fire the telemetry.
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

        // Only the earliest offender is kept.
        let kept = before.first_misaligned.or(Some((7, 3, 0x2000)));
        assert_eq!(after.first_misaligned, kept, "the earliest offender wins");
    }

    #[test]
    fn the_trampoline_has_a_real_address() {
        // A zero here would make every thunk jump to the null page.
        assert_ne!(trampoline_address(), 0);
    }

    #[test]
    fn six_argument_registers_are_saved() {
        // System V passes six integers in registers; the seventh onwards is on the stack and not
        // captured.
        assert_eq!(SAVED_ARGUMENT_REGISTERS, 6);
    }

    #[test]
    fn the_ring_is_bounded_so_the_call_path_never_allocates() {
        // A sink that allocates on a guest thread changes the program it observes (D018).
        const { assert!(MAX_RECORDED_CALLS > 0) }
        assert!(MAX_RECORDED_CALLS.is_power_of_two());
    }
}
