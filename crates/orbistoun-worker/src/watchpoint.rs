//! Trapping on an access to guest memory, and saying which instruction made it.
//!
//! The expensive half of the pair [`crate::watch`] describes. A snapshot says *which bytes
//! ended up different*; this says *which instruction touched them, and how often*. Both are
//! kept, because they answer different questions and the cheap one is still the one to run
//! first (D223, D276).
//!
//! # What this is for
//!
//! The question at the `image+0xafc959` wall is no longer "did anything fill this slot in?".
//! The snapshot answered that, and the answer was no. It is "**who read the slot nobody
//! filled?**", and only a trap per access can answer it.
//!
//! # The two compose
//!
//! Run the snapshot first: it names every word in a structure that nobody wrote. Take up to
//! four of those addresses and arm them here on the next run. Each hit reports the
//! instruction that consumed the empty slot, its offset in a named region, and the value it
//! saw. Neither step reads the guest's code, which is what makes the pipeline mechanical.
//!
//! # What the hardware costs
//!
//! Four watchpoints, of one, two, four or eight bytes, each aligned to its own length. Those
//! are properties of x86 debug registers rather than choices made here, so a request that
//! breaks one is **refused with the reason** rather than quietly rounded into something that
//! would watch the wrong bytes.
//!
//! # A trap, not a fault
//!
//! A data breakpoint fires *after* the access completes, so the instruction pointer belongs
//! to the **next** instruction. Naming the one that actually did it needs its length, and
//! length comes from decoding it - which is disassembly of a vendor binary, refused by
//! principle 1. So every line here says `after the access at`, never `at` (D277).

use core::sync::atomic::{AtomicU64, Ordering};

use orbistoun_report::trace::Registers;

/// How many the hardware has. Not a tunable.
pub const MAX_WATCHPOINTS: usize = 4;

/// How many distinct instruction sites are remembered before hits are only counted.
///
/// A watched word inside a loop is touched thousands of times from a handful of places, so
/// the interesting quantity is the set of places rather than the stream of accesses. Sites
/// beyond this are counted by [`dropped`] rather than silently discarded.
const MAX_SITES: usize = 32;

/// What kind of access is trapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Only writes. Answers "who filled this in?".
    Write,
    /// Reads or writes. Answers "who consumed this?" - the question at the wall.
    ///
    /// x86 has no read-only encoding, so asking for reads gets writes as well. Said here
    /// rather than discovered from a report with more lines in it than expected.
    Access,
    /// Execution reaching an address, answering "what were the arguments when this was called?".
    ///
    /// The one kind that watches *code* rather than data. It fires **before** the instruction
    /// runs, so it captures the register state a function is entered with - which is how the
    /// value a guest computes and hands to something like `tlsf_add_pool` becomes readable
    /// without disassembling the guest to find where it came from (D458). It is **one-shot**:
    /// the first hit snapshots the registers and then disarms itself, so the instruction runs
    /// and the guest carries on - no resume-flag or single-step dance, and no infinite re-trap.
    Execute,
}

impl Kind {
    /// The `R/W` field the debug-control register wants.
    const fn bits(self) -> u64 {
        match self {
            // Execute is `0b00`: the debug register breaks on an instruction fetch at the
            // address rather than a data access to it.
            Self::Execute => 0b00,
            Self::Write => 0b01,
            Self::Access => 0b11,
        }
    }

    /// How it is written in a report.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Access => "read-or-write",
            Self::Execute => "execute",
        }
    }

    /// How it is spelled in a request.
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "w" | "write" => Some(Self::Write),
            "rw" | "access" => Some(Self::Access),
            "x" | "execute" => Some(Self::Execute),
            _ => None,
        }
    }
}

/// One address to trap on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// Where. Must be aligned to `length`.
    pub address: u64,
    /// One, two, four or eight bytes.
    pub length: u64,
    /// Writes only, or reads as well.
    pub kind: Kind,
}

impl Request {
    /// The `LEN` field the debug-control register wants.
    ///
    /// The encoding is not sequential - four bytes is `0b11` and eight is `0b10` - which is
    /// exactly the sort of table worth writing out rather than computing.
    const fn length_bits(self) -> Option<u64> {
        match self.length {
            1 => Some(0b00),
            2 => Some(0b01),
            4 => Some(0b11),
            8 => Some(0b10),
            _ => None,
        }
    }

    /// Why the hardware would refuse this, if it would.
    fn objection(self) -> Option<String> {
        if self.length_bits().is_none() {
            return Some(format!(
                "{:#x}+{}: a watchpoint covers one, two, four or eight bytes",
                self.address, self.length
            ));
        }
        if self.address % self.length != 0 {
            return Some(format!(
                "{:#x}+{}: an {}-byte watchpoint needs an {}-byte-aligned address",
                self.address, self.length, self.length, self.length
            ));
        }
        None
    }
}

/// Reads a list of requests, or says what is wrong with it.
///
/// `<addr>[+len][:kind]`, comma-separated. Length defaults to eight, because a guest
/// structure is made of words and that is the shape the snapshot reports. Kind defaults to
/// `rw`, because "who consumed this?" is the question this exists for.
///
/// **Refuses rather than skips.** A watchpoint that was requested and not armed reports a
/// run under a diagnostic that did nothing, which is the failure every diagnostic in this
/// crate is built to avoid (D185, D218).
pub fn parse(raw: &str) -> Result<Vec<Request>, String> {
    let mut requests = Vec::new();
    for field in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (site, kind) = match field.split_once(':') {
            Some((site, kind)) => (
                site,
                Kind::parse(kind.trim())
                    .ok_or_else(|| format!("{kind}: a watchpoint is w, rw or x"))?,
            ),
            None => (field, Kind::Access),
        };
        let (address, length) = match site.split_once('+') {
            Some((address, length)) => (address, number(length)?),
            None => (site, 8),
        };
        // An execute breakpoint watches an instruction fetch, which the hardware encodes as
        // LEN=00 - one byte - whatever the instruction's real length. Force it, so a caller
        // need not know the encoding and a stray `+len` cannot ask for something the mode forbids.
        let length = if kind == Kind::Execute { 1 } else { length };
        let request = Request {
            address: number(address)?,
            length,
            kind,
        };
        if let Some(objection) = request.objection() {
            return Err(objection);
        }
        requests.push(request);
    }
    if requests.len() > MAX_WATCHPOINTS {
        return Err(format!(
            "{} requested; the hardware has {MAX_WATCHPOINTS}",
            requests.len()
        ));
    }
    Ok(requests)
}

/// A number, decimal or hexadecimal.
fn number(raw: &str) -> Result<u64, String> {
    let text = raw.trim();
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| format!("{text}: not a number"))
}

// --- What was armed, so a hit can be described -------------------------------

/// Address of each armed watchpoint, or zero.
static ARMED_ADDRESS: [AtomicU64; MAX_WATCHPOINTS] = [const { AtomicU64::new(0) }; MAX_WATCHPOINTS];
/// Length of each, parallel to [`ARMED_ADDRESS`].
static ARMED_LENGTH: [AtomicU64; MAX_WATCHPOINTS] = [const { AtomicU64::new(0) }; MAX_WATCHPOINTS];
/// One when the slot is an execute breakpoint, zero for a data one, parallel to
/// [`ARMED_ADDRESS`]. An execute hit snapshots registers and disarms; a data hit records the
/// accessing instruction. The two are told apart here rather than by re-deriving from `Dr7`.
static ARMED_EXECUTE: [AtomicU64; MAX_WATCHPOINTS] = [const { AtomicU64::new(0) }; MAX_WATCHPOINTS];

// --- The one-shot execute snapshot -------------------------------------------

/// The address of the first execute-breakpoint hit, plus one so zero means "not yet hit".
static EXEC_RIP: AtomicU64 = AtomicU64::new(0);
/// The sixteen registers captured at that first hit, in [`Registers`] field order.
static EXEC_REGS: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
/// How many times any execute breakpoint fired, which says whether the first hit was the only
/// one - a function called once versus a hot one.
static EXEC_COUNT: AtomicU64 = AtomicU64::new(0);

/// Records an execute-breakpoint hit: counts it, and on the first snapshots the registers the
/// instruction was about to run with.
///
/// Allocation-free (atomics only), because this runs on the guest's own stack from the
/// exception handler. `compare_exchange` on the rip makes exactly one caller the first, so the
/// registers stored are the ones that belong to the rip stored (D458).
fn note_execute(rip: u64, registers: &Registers) {
    use Ordering::{Relaxed, Release};
    EXEC_COUNT.fetch_add(1, Relaxed);
    if EXEC_RIP
        .compare_exchange(0, rip.wrapping_add(1), Release, Relaxed)
        .is_ok()
    {
        let all = [
            registers.rax,
            registers.rbx,
            registers.rcx,
            registers.rdx,
            registers.rsi,
            registers.rdi,
            registers.rbp,
            registers.rsp,
            registers.r8,
            registers.r9,
            registers.r10,
            registers.r11,
            registers.r12,
            registers.r13,
            registers.r14,
            registers.r15,
        ];
        for (slot, value) in all.iter().enumerate() {
            EXEC_REGS[slot].store(*value, Relaxed);
        }
    }
}

/// Records what is being watched, so the handler can describe a hit without allocating.
fn remember(requests: &[Request]) {
    for (slot, request) in requests.iter().enumerate().take(MAX_WATCHPOINTS) {
        ARMED_ADDRESS[slot].store(request.address, Ordering::Relaxed);
        ARMED_LENGTH[slot].store(request.length, Ordering::Relaxed);
        ARMED_EXECUTE[slot].store(u64::from(request.kind == Kind::Execute), Ordering::Relaxed);
    }
}

/// Whether anything is armed, so an ordinary run says nothing about watchpoints.
pub fn armed() -> bool {
    ARMED_ADDRESS.iter().any(|a| a.load(Ordering::Relaxed) != 0)
}

// --- Where hits are recorded -------------------------------------------------

/// Which watchpoint the site belongs to, plus one. Zero means the slot is free.
static SITE_SLOT: [AtomicU64; MAX_SITES] = [const { AtomicU64::new(0) }; MAX_SITES];
/// The instruction pointer *after* the access (D277).
static SITE_AFTER: [AtomicU64; MAX_SITES] = [const { AtomicU64::new(0) }; MAX_SITES];
/// What the watched word held the first time this site touched it.
static SITE_FIRST: [AtomicU64; MAX_SITES] = [const { AtomicU64::new(0) }; MAX_SITES];
/// What it held most recently.
static SITE_LAST: [AtomicU64; MAX_SITES] = [const { AtomicU64::new(0) }; MAX_SITES];
/// How many times this site touched it.
static SITE_COUNT: [AtomicU64; MAX_SITES] = [const { AtomicU64::new(0) }; MAX_SITES];
/// Accesses from a site the table had no room for. **Reported, never silent.**
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Traps that arrived without the platform saying which watchpoint fired.
///
/// The debug-status register reaches a handler through the exception context, and whether
/// that context carries the debug registers is the platform's business rather than ours. If
/// it ever does not, the access is real and the attribution is missing - which is a thing to
/// **say**, not a thing to guess at by picking the first armed slot.
static UNATTRIBUTED: AtomicU64 = AtomicU64::new(0);

/// Accesses that arrived from more distinct instructions than the table holds.
pub fn dropped() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

/// Reads the watched bytes, for a report that says what the instruction saw.
///
/// Zero-extended when fewer than eight, which is what a one-, two- or four-byte watchpoint
/// is asking about anyway.
fn value_at(address: u64, length: u64) -> u64 {
    let Ok(at) = usize::try_from(address) else {
        return 0;
    };
    let Ok(take) = usize::try_from(length) else {
        return 0;
    };
    let pointer = std::ptr::with_exposed_provenance::<u8>(at);
    let wide = take.min(8);
    // SAFETY: this is the address the arming step put in a debug register, and the hardware
    // has just trapped an access to it - so the bytes are mapped, and a region accessed a
    // moment ago cannot have been unmapped between the trap and here.
    let seen = unsafe { std::slice::from_raw_parts(pointer, wide) };
    let mut bytes = [0_u8; 8];
    bytes[..wide].copy_from_slice(seen);
    u64::from_le_bytes(bytes)
}

/// Records one access, and says whether it came from a site never seen before.
///
/// **Allocation-free**, because this runs from an exception handler on the guest's own
/// stack, where allocating risks deadlocking against whatever was interrupted - the same
/// rule the fault reporter already works under.
fn record(slot: usize, after: u64) -> bool {
    let tag = slot as u64 + 1;
    let value = value_at(
        ARMED_ADDRESS[slot].load(Ordering::Relaxed),
        ARMED_LENGTH[slot].load(Ordering::Relaxed),
    );
    for index in 0..MAX_SITES {
        let held = SITE_SLOT[index].load(Ordering::Relaxed);
        if held == tag && SITE_AFTER[index].load(Ordering::Relaxed) == after {
            SITE_COUNT[index].fetch_add(1, Ordering::Relaxed);
            SITE_LAST[index].store(value, Ordering::Relaxed);
            return false;
        }
        if held == 0 {
            SITE_AFTER[index].store(after, Ordering::Relaxed);
            SITE_FIRST[index].store(value, Ordering::Relaxed);
            SITE_LAST[index].store(value, Ordering::Relaxed);
            SITE_COUNT[index].store(1, Ordering::Relaxed);
            // Last, so a reader of the table never sees a claimed slot whose fields are
            // still the previous run's zeros.
            SITE_SLOT[index].store(tag, Ordering::Relaxed);
            return true;
        }
    }
    DROPPED.fetch_add(1, Ordering::Relaxed);
    false
}

/// Every site that touched a watched address, as lines a person reads.
///
/// Ordered by watchpoint and then by discovery, so the first line for an address is the
/// first instruction that ever touched it - which is the one the question is usually about.
pub fn sites() -> Vec<String> {
    let mut lines = Vec::new();
    for (slot, armed_at) in ARMED_ADDRESS.iter().enumerate() {
        let address = armed_at.load(Ordering::Relaxed);
        if address == 0 {
            continue;
        }
        let tag = slot as u64 + 1;
        let mut any = false;
        for index in 0..MAX_SITES {
            if SITE_SLOT[index].load(Ordering::Relaxed) != tag {
                continue;
            }
            any = true;
            let after = SITE_AFTER[index].load(Ordering::Relaxed);
            let first = SITE_FIRST[index].load(Ordering::Relaxed);
            let last = SITE_LAST[index].load(Ordering::Relaxed);
            let count = SITE_COUNT[index].load(Ordering::Relaxed);
            let saw = if first == last {
                format!("saw {first:#x}")
            } else {
                format!("saw {first:#x} then {last:#x}")
            };
            let times = if count > 1 {
                format!(", {count} times")
            } else {
                String::new()
            };
            lines.push(format!(
                "  {address:#x}: after the access at {}, {saw}{times}",
                located(after)
            ));
        }
        if !any {
            // As interesting as a hit. A watched word nothing ever touched says the guest is
            // not reading the field the hypothesis was about, and omitting the line would
            // read as a diagnostic that had not run (D218).
            lines.push(format!("  {address:#x}: never touched"));
        }
    }
    let unattributed = UNATTRIBUTED.load(Ordering::Relaxed);
    if unattributed > 0 {
        lines.push(format!(
            "  {unattributed} accesses were trapped without the platform saying which watchpoint fired"
        ));
    }
    let lost = dropped();
    if lost > 0 {
        lines.push(format!(
            "  {lost} further accesses came from more than {MAX_SITES} distinct instructions and were not recorded"
        ));
    }
    lines
}

/// Whether the summary has already been printed.
///
/// A run ends once, but the trace is persisted from whichever path got there - a fault, the
/// clock, the call budget, or an ordinary return - and more than one can fire as a process
/// comes apart. One summary, from whichever arrives first.
static SUMMARISED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Prints what every watchpoint saw, once, at whatever ends the run.
///
/// Silent when nothing was armed *and* no execute breakpoint fired - an ordinary run reads
/// exactly as it did before. An execute breakpoint disarms itself on its one hit, so `armed()`
/// is false by the end; the snapshot it left is what says it did anything, so it is checked too.
pub fn summarise() {
    use std::io::Write as _;

    let fired_execute = EXEC_RIP.load(Ordering::Relaxed) != 0;
    if (!armed() && !fired_execute) || SUMMARISED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut lines = vec!["orbistoun: watchpoints".to_owned()];
    lines.extend(sites());
    lines.extend(execute_snapshot());
    lines.push(String::new());
    let _ = std::io::stderr().write_all(lines.join("\n").as_bytes());
}

/// The register snapshot an execute breakpoint captured, as lines, or nothing if none fired.
///
/// The point of the whole execute mode: the state a function was entered with, so the value a
/// guest computed and passed - the size to `tlsf_add_pool`, say - is read straight off the
/// arguments rather than chased back through code that cannot be disassembled (D458).
fn execute_snapshot() -> Vec<String> {
    let rip = EXEC_RIP.load(Ordering::Relaxed);
    if rip == 0 {
        return Vec::new();
    }
    let registers = Registers {
        rax: EXEC_REGS[0].load(Ordering::Relaxed),
        rbx: EXEC_REGS[1].load(Ordering::Relaxed),
        rcx: EXEC_REGS[2].load(Ordering::Relaxed),
        rdx: EXEC_REGS[3].load(Ordering::Relaxed),
        rsi: EXEC_REGS[4].load(Ordering::Relaxed),
        rdi: EXEC_REGS[5].load(Ordering::Relaxed),
        rbp: EXEC_REGS[6].load(Ordering::Relaxed),
        rsp: EXEC_REGS[7].load(Ordering::Relaxed),
        r8: EXEC_REGS[8].load(Ordering::Relaxed),
        r9: EXEC_REGS[9].load(Ordering::Relaxed),
        r10: EXEC_REGS[10].load(Ordering::Relaxed),
        r11: EXEC_REGS[11].load(Ordering::Relaxed),
        r12: EXEC_REGS[12].load(Ordering::Relaxed),
        r13: EXEC_REGS[13].load(Ordering::Relaxed),
        r14: EXEC_REGS[14].load(Ordering::Relaxed),
        r15: EXEC_REGS[15].load(Ordering::Relaxed),
    };
    let count = EXEC_COUNT.load(Ordering::Relaxed);
    let mut lines = vec![format!(
        "  execute breakpoint at {} hit {count} time(s); registers the first time (arguments in \
         rdi, rsi, rdx, rcx, r8, r9):",
        located(rip.wrapping_sub(1))
    )];
    lines.extend(
        registers
            .lines()
            .into_iter()
            .map(|line| format!("  {line}")),
    );
    lines
}

/// An address, with the region it falls in when that is known.
fn located(address: u64) -> String {
    match crate::report::locate(address) {
        Some((name, offset)) => format!("{address:#x} ({name}+{offset:#x})"),
        None => format!("{address:#x}"),
    }
}

// --- Arming, and the exception that follows ----------------------------------

/// Arms the requested watchpoints on the thread that is about to become the guest.
///
/// Returns what was armed, in the words the run conditions will carry. The caller states the
/// outcome out loud either way, because a diagnostic that was asked for and did not run must
/// never read like an ordinary run.
pub fn arm(requests: &[Request]) -> Result<String, String> {
    if requests.is_empty() {
        return Ok(String::new());
    }
    imp::arm(requests)?;
    remember(requests);
    let described: Vec<String> = requests
        .iter()
        .map(|r| format!("{:#x}+{} {}", r.address, r.length, r.kind.label()))
        .collect();
    Ok(described.join(", "))
}

/// What a debug exception was, and what the handler must do to resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    /// Not one of ours - a debugger's or a runtime's. Let another handler see it.
    NotOurs,
    /// A data watchpoint fired; the access is recorded and the guest resumes unchanged.
    Data,
    /// One or more execute breakpoints fired; the guest resumes, and the handler must clear
    /// these `Dr7` enable bits so the one-shot does not re-trap the same instruction forever.
    Execute {
        /// The bits to clear in `Dr7` - the `L` enable bit of each fired execute slot.
        disarm: u64,
    },
}

/// Handles a debug exception: records the access and lets the guest carry on.
///
/// `debug_status` is the debug-status register, whose low four bits say which watchpoints
/// fired - and more than one can, for a single access two of them cover. `after` is the
/// instruction pointer the exception carried: the *next* instruction for a data watchpoint
/// (which fires after the access), and the breakpoint instruction itself for an execute one
/// (which fires before it). `registers` is the guest state, snapshotted for an execute hit.
///
/// The guest **continues**. Unlike an access violation, where carrying on would leave a guest
/// running with corrupt state, a trapped access has already happened and everything is intact.
pub fn note(debug_status: u64, after: u64, registers: &Registers) -> Trap {
    /// The low four bits, one per watchpoint, saying which of them fired.
    const FIRED: u64 = 0b1111;

    if !armed() {
        // Not ours. Debuggers and language runtimes raise debug exceptions routinely, and
        // swallowing one that belongs to somebody else would break them.
        return Trap::NotOurs;
    }
    // **The handler's own read of the watched word is a watched access.** A debug register
    // stays live while its handler runs, and x86 sets the resume flag only for instruction
    // breakpoints - so reading the word to say what the instruction saw traps, which reads
    // it again, until the process dies having reported nothing (D278).
    //
    // Still ours, so the nested trap resumes the guest rather than falling through to the
    // next handler. Released at the end of the outermost call.
    if REENTERED.swap(true, Ordering::Relaxed) {
        return Trap::Data;
    }
    let (ours, disarm) = attribute(debug_status & FIRED, after, registers);
    REENTERED.store(false, Ordering::Relaxed);
    if !ours {
        Trap::NotOurs
    } else if disarm != 0 {
        Trap::Execute { disarm }
    } else {
        Trap::Data
    }
}

/// Whether the handler is already running, so its own reads do not re-enter it.
///
/// A flag rather than clearing and restoring the control register around the read: writing
/// the register back can fail halfway, which would leave a run with watchpoints that had
/// silently stopped working - the same shape of wrong answer this whole module exists to
/// refuse. A flag cannot half-succeed (D278).
static REENTERED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Charges one trap to the watchpoints that fired. Answers `(ours, disarm)`: whether any were
/// ours, and the `Dr7` enable bits of the fired execute slots the handler must clear.
///
/// Split out so the re-entrancy flag in [`note`] has exactly one place to be released, rather
/// than one per early return.
fn attribute(fired: u64, after: u64, registers: &Registers) -> (bool, u64) {
    if fired == 0 {
        UNATTRIBUTED.fetch_add(1, Ordering::Relaxed);
        return (true, 0);
    }
    let mut ours = false;
    let mut disarm = 0;
    for (slot, address) in ARMED_ADDRESS.iter().enumerate() {
        if fired >> slot & 1 == 0 || address.load(Ordering::Relaxed) == 0 {
            continue;
        }
        ours = true;
        if ARMED_EXECUTE[slot].load(Ordering::Relaxed) == 1 {
            // Execute: snapshot the state the instruction was entered with, then mark the slot
            // for disarming and forget it, so this is a one-shot - the instruction runs, the
            // guest carries on, and neither a later trap nor the summary sees it twice.
            note_execute(after, registers);
            disarm |= 1 << (slot * 2);
            address.store(0, Ordering::Relaxed);
        } else if record(slot, after) {
            announce(slot, after);
        }
    }
    (ours, disarm)
}

/// Says a new instruction touched a watched address, as it happens.
///
/// Printed live rather than only summarised, because the run may end in a fault that kills
/// the process, and a finding that only exists in a summary nobody reaches is not a finding.
/// Bounded by the de-duplication in [`record`]: one line per instruction, not per access.
fn announce(slot: usize, after: u64) {
    use std::io::Write as _;

    /// Written explicitly so the line survives however the stream is buffered.
    const NEWLINE: &str = "\n";

    let address = ARMED_ADDRESS[slot].load(Ordering::Relaxed);
    let length = ARMED_LENGTH[slot].load(Ordering::Relaxed);
    let mut line = crate::report::Line::new();
    line.text("orbistoun: watchpoint ")
        .address(address)
        .text(" touched after the access at ")
        .address(after)
        .text("; it now holds ")
        .hex(value_at(address, length))
        .text(NEWLINE);
    let _ = std::io::stderr().write_all(line.as_bytes());
}

#[cfg(windows)]
mod imp {
    use super::{MAX_WATCHPOINTS, Request};

    /// Everything below the per-watchpoint fields.
    ///
    /// Bits eight and nine are `LE`/`GE`, which older parts wanted set for data breakpoints
    /// to be reported exactly. Modern ones ignore them; setting them costs nothing and means
    /// the value does not depend on which part it runs on.
    const DR7_EXACT: u64 = 0x0000_0300;

    /// The flag that says only the debug registers are being read or written.
    ///
    /// Written out rather than imported: a context flag naming the wrong subset silently
    /// writes the wrong half of a thread's state, and a constant that is visible is a
    /// constant that can be checked against the manual.
    const CONTEXT_DEBUG_REGISTERS: u32 = 0x0010_0010;

    /// Duplicate the handle with the same access the source has.
    const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;

    /// What `SuspendThread` and `ResumeThread` return when they fail.
    const SUSPEND_FAILED: u32 = u32::MAX;

    /// The debug-control value that arms these requests and nothing else.
    pub(super) fn control(requests: &[Request]) -> u64 {
        let mut value = DR7_EXACT;
        for (slot, request) in requests.iter().enumerate().take(MAX_WATCHPOINTS) {
            let shift = slot * 4;
            value |= 1 << (slot * 2);
            value |= request.kind.bits() << (16 + shift);
            value |= request.length_bits().unwrap_or(0) << (18 + shift);
        }
        value
    }

    /// Sets the debug registers on the calling thread.
    ///
    /// # Why another thread does it
    ///
    /// `SetThreadContext` on a running thread is only defined when that thread is suspended,
    /// and a thread cannot suspend itself. So a helper is spawned to suspend this one, write
    /// the registers, and resume it - the textbook shape, rather than the widely-copied
    /// version that sets its own context and works until it does not.
    pub(super) fn arm(requests: &[Request]) -> Result<String, String> {
        use windows_sys::Win32::Foundation::{CloseHandle, DuplicateHandle};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread};

        let control = control(requests);
        let mut addresses = [0_u64; MAX_WATCHPOINTS];
        for (slot, request) in requests.iter().enumerate().take(MAX_WATCHPOINTS) {
            addresses[slot] = request.address;
        }

        // SAFETY: the pseudo-handle for the calling process, which is always valid.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: the pseudo-handle for the calling thread, which is valid in this thread -
        // and this thread is the one about to become the guest, which is the point.
        let thread = unsafe { GetCurrentThread() };

        let mut duplicated = std::ptr::null_mut();
        // SAFETY: both source handles were taken above and are valid here, and the
        // out-parameter is a live local of the right type.
        let copied = unsafe {
            DuplicateHandle(
                process,
                thread,
                process,
                &raw mut duplicated,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if copied == 0 || duplicated.is_null() {
            return Err("could not take a handle to the thread about to run the guest".to_owned());
        }

        // Carried as a number because a raw handle is not `Send`. It is a value the operating
        // system owns rather than a pointer this process may dereference, so moving it across
        // a thread boundary as an integer is exactly what it is.
        let carried = duplicated.expose_provenance();
        let outcome = std::thread::spawn(move || write_registers(carried, control, addresses))
            .join()
            .unwrap_or_else(|_| Err("the arming thread died".to_owned()));

        // SAFETY: `duplicated` came from `DuplicateHandle` above, has not been closed, and
        // the arming thread has been joined so nothing else holds it.
        unsafe {
            CloseHandle(duplicated);
        }
        outcome
    }

    /// Suspends the target, writes its debug registers, and resumes it.
    fn write_registers(
        thread: usize,
        control: u64,
        addresses: [u64; MAX_WATCHPOINTS],
    ) -> Result<String, String> {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            CONTEXT, GetThreadContext, SetThreadContext,
        };
        use windows_sys::Win32::System::Threading::{ResumeThread, SuspendThread};

        let handle = std::ptr::with_exposed_provenance_mut::<core::ffi::c_void>(thread);

        // SAFETY: a live thread handle taken by `DuplicateHandle`, kept open by the caller
        // until this function has been joined.
        if unsafe { SuspendThread(handle) } == SUSPEND_FAILED {
            return Err("could not suspend the thread to set its debug registers".to_owned());
        }

        // SAFETY: `CONTEXT` is plain data with no invalid bit patterns, and every field read
        // below is one the call is about to fill in.
        let mut context: CONTEXT = unsafe { core::mem::zeroed() };
        context.ContextFlags = CONTEXT_DEBUG_REGISTERS;

        // SAFETY: the thread is suspended, the handle is live, and the context is a live
        // local of the right type with its flags set to the subset being read.
        let read = unsafe { GetThreadContext(handle, &raw mut context) };
        if read == 0 {
            // SAFETY: as above. Resuming matters more than the error being returned, because
            // a thread left suspended hangs the run with nothing said.
            unsafe { ResumeThread(handle) };
            return Err("could not read the thread's debug registers".to_owned());
        }

        context.Dr0 = addresses[0];
        context.Dr1 = addresses[1];
        context.Dr2 = addresses[2];
        context.Dr3 = addresses[3];
        context.Dr6 = 0;
        context.Dr7 = control;
        context.ContextFlags = CONTEXT_DEBUG_REGISTERS;

        // SAFETY: as for the read; the thread is still suspended and the context is the one
        // just read back, with only debug-register fields changed.
        let written = unsafe { SetThreadContext(handle, &raw const context) };

        // SAFETY: the handle is live and the thread was suspended by this function.
        let resumed = unsafe { ResumeThread(handle) };
        if written == 0 {
            return Err("could not set the thread's debug registers".to_owned());
        }
        if resumed == SUSPEND_FAILED {
            return Err("the thread could not be resumed after arming".to_owned());
        }
        Ok(String::new())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Request;

    /// Not implemented away from Windows yet.
    ///
    /// The equivalent is `ptrace(PTRACE_POKEUSER)` into the debug-register area, which needs
    /// a tracer process rather than a thread arming itself. Saying so plainly beats a
    /// function that returns success and arms nothing - which is the one outcome a
    /// diagnostic must never have (D185).
    pub(super) fn arm(_requests: &[Request]) -> Result<String, String> {
        Err("watchpoints need debug registers, which are only wired up on Windows".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, MAX_WATCHPOINTS, parse};

    #[test]
    fn an_address_alone_watches_a_word_for_reads_and_writes() {
        let parsed = parse("0x4000019e9c00").expect("a bare address is a request");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].address, 0x4000_019e_9c00);
        assert_eq!(parsed[0].length, 8);
        assert_eq!(parsed[0].kind, Kind::Access);
    }

    #[test]
    fn a_length_and_a_kind_are_both_read() {
        let parsed = parse("0x1000+4:w").expect("length and kind are optional, not exclusive");
        assert_eq!(parsed[0].length, 4);
        assert_eq!(parsed[0].kind, Kind::Write);
    }

    #[test]
    fn several_are_separated_by_commas() {
        let parsed = parse("0x1000, 0x2000+1:w ,0x3008").expect("a list, spaces and all");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[2].address, 0x3008);
    }

    #[test]
    fn an_unaligned_address_is_refused_with_the_reason() {
        // The hardware would watch a different eight bytes than the ones asked for, and a
        // diagnostic watching the wrong address answers the question confidently and wrongly.
        let refused = parse("0x1004").expect_err("an eight-byte watch needs eight-byte alignment");
        assert!(refused.contains("aligned"), "{refused}");
    }

    #[test]
    fn a_length_the_hardware_cannot_encode_is_refused() {
        let refused = parse("0x1000+16").expect_err("sixteen bytes is not a watchpoint");
        assert!(refused.contains("one, two, four or eight"), "{refused}");
    }

    #[test]
    fn more_than_the_hardware_has_is_refused_rather_than_truncated() {
        // Truncating would arm four of five and report as though all five had run.
        let refused =
            parse("0x1000,0x2000,0x3000,0x4000,0x5000").expect_err("five into four does not go");
        assert!(refused.contains(&MAX_WATCHPOINTS.to_string()), "{refused}");
    }

    #[test]
    fn nothing_requested_is_no_requests_rather_than_an_error() {
        let parsed = parse("").expect("an unset variable is not a mistake");
        assert!(parsed.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn the_control_word_matches_the_manual() {
        use super::Request;
        use super::imp::control;

        // One eight-byte read-or-write watchpoint in slot zero: local enable in bit 0,
        // `R/W0 = 0b11` at bit 16, `LEN0 = 0b10` at bit 18, over the exact-match bits.
        let one = control(&[Request {
            address: 0x1000,
            length: 8,
            kind: Kind::Access,
        }]);
        assert_eq!(one, 0x0000_0300 | 1 | (0b11 << 16) | (0b10 << 18));

        // A four-byte write watchpoint in slot one lands four bits further up, and four
        // bytes encodes as `0b11` rather than `0b10` - the part of the table worth a test.
        let two = control(&[
            Request {
                address: 0x1000,
                length: 8,
                kind: Kind::Access,
            },
            Request {
                address: 0x2000,
                length: 4,
                kind: Kind::Write,
            },
        ]);
        assert_eq!(two, one | (1 << 2) | (0b01 << 20) | (0b11 << 22));
    }

    /// An execute breakpoint is spelled `:x`, and its length is forced to one byte whatever was
    /// asked - the hardware watches an instruction fetch as LEN=00 regardless of instruction size.
    #[test]
    fn an_execute_breakpoint_is_one_byte_however_it_is_asked() {
        let parsed = parse("0x400000afcc08:x").expect("an execute breakpoint");
        assert_eq!(parsed[0].kind, Kind::Execute);
        assert_eq!(parsed[0].length, 1);
        // A stray length is overridden rather than refused: the mode has only one.
        let anyway = parse("0x1000+8:x").expect("length is ignored for execute, not an error");
        assert_eq!(anyway[0].length, 1);
    }

    /// **An execute breakpoint encodes as R/W=00, LEN=00** - the part of the control word a
    /// data watchpoint never exercises, and the one a typo would turn into a data breakpoint on
    /// the code, which watches the wrong thing silently.
    #[cfg(windows)]
    #[test]
    fn an_execute_breakpoint_encodes_as_a_fetch() {
        use super::Request;
        use super::imp::control;

        let encoded = control(&[Request {
            address: 0x1000,
            length: 1,
            kind: Kind::Execute,
        }]);
        // Local enable in bit 0, R/W0 = 0b00 at bit 16, LEN0 = 0b00 at bit 18, over the
        // exact-match bits - i.e. nothing but the enable above the base.
        assert_eq!(encoded, 0x0000_0300 | 1);
    }
}
