//! Saying *where* a guest faulted, from inside the fault.
//!
//! An access violation with no address is one bit of information. The same fault with
//! "read of `0x0` while executing at image+0x1a4c20" is a work list - it says whether
//! the guest dereferenced a null thread pointer, ran off the end of a stub, or jumped
//! somewhere that was never mapped.
//!
//! # Why this writes to the error stream and not the protocol stream
//!
//! The protocol is newline-delimited JSON, and a process dying mid-write would leave a
//! half-finished line that breaks the reader for good. A fault report is diagnostic
//! text, so it goes to the error stream where a truncated line costs nothing. The
//! parent still produces a structured verdict of its own (D064); this adds the detail
//! that only the faulting process can know.
//!
//! # The formatting is allocation-free
//!
//! A handler runs on a thread that has just faulted, on the guest stack. Building the
//! message with the ordinary formatting machinery would allocate, and allocating there
//! risks deadlocking against the code that crashed. So the message is assembled in a
//! fixed buffer and issued as a single write - the same rule principle 9 already
//! imposes on trace recording.

use core::sync::atomic::{AtomicU64, Ordering};

// The shapes this fills in live one layer down, in `orbistoun-report`, so the service
// layer and both shims can see them. Only the producing side is here (D160).
use orbistoun_report::trace::{
    AbiReport, ArgumentDump, CallTrace, CalledImport, Conditions, FaultSite, FormatReport, Frame,
    ReadReport, Registers, TAIL_CALLS, TracedCall,
};

/// How many regions can be named in a fault report.
const MAX_REGIONS: usize = 4;

/// Bases of the named regions, or zero for an unused slot.
static REGION_BASE: [AtomicU64; MAX_REGIONS] = [const { AtomicU64::new(0) }; MAX_REGIONS];
/// Lengths, parallel to [`REGION_BASE`].
static REGION_LEN: [AtomicU64; MAX_REGIONS] = [const { AtomicU64::new(0) }; MAX_REGIONS];

/// Names, indexed the same way. Fixed rather than stored, so the handler never chases a
/// pointer that the faulting code may have invalidated.
const REGION_NAMES: [&str; MAX_REGIONS] = ["image", "stubs", "stack", "other"];

/// Which slot each kind of region occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// The placed guest image.
    Image,
    /// The per-import stub table.
    Stubs,
    /// The guest stack.
    Stack,
}

impl Region {
    /// Slot index for this region.
    const fn slot(self) -> usize {
        match self {
            Self::Image => 0,
            Self::Stubs => 1,
            Self::Stack => 2,
        }
    }
}

/// Registers a region so a fault inside it can be named rather than left as a number.
///
/// Call before entering the guest. Registering afterwards would be too late for the
/// only fault that matters.
pub fn describe_region(region: Region, base: u64, len: u64) {
    REGION_BASE[region.slot()].store(base, Ordering::Relaxed);
    REGION_LEN[region.slot()].store(len, Ordering::Relaxed);
}

/// Names the region containing `address`, and the offset into it.
///
/// Pure and therefore testable, which matters more than usual here: the handler that
/// uses it cannot be stepped through, and a bug in it appears as a garbled message at
/// the exact moment the message is most needed.
pub fn locate(address: u64) -> Option<(&'static str, u64)> {
    (0..MAX_REGIONS).find_map(|i| {
        let base = REGION_BASE[i].load(Ordering::Relaxed);
        let len = REGION_LEN[i].load(Ordering::Relaxed);
        (len > 0 && address >= base && address < base.saturating_add(len))
            .then(|| (REGION_NAMES[i], address - base))
    })
}

/// A fixed-size line builder.
///
/// No allocation and no locks, because a handler may run while the allocator lock is
/// held by the code that just crashed.
#[derive(Debug)]
pub struct Line {
    buffer: [u8; Self::CAPACITY],
    used: usize,
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl Line {
    /// Longest report this can hold. Anything beyond is dropped rather than wrapped.
    ///
    /// Raised from 512 when the report began carrying the faulting instruction's bytes and the
    /// window before it: two hex runs cost ~200 characters, and the register and frame lines that
    /// follow them are the ones that must never be the part that gets dropped. A stack buffer in a
    /// fault handler, so the room is close to free.
    pub const CAPACITY: usize = 1024;

    /// An empty line.
    pub const fn new() -> Self {
        Self {
            buffer: [0; Self::CAPACITY],
            used: 0,
        }
    }

    /// Appends text, truncating rather than overflowing.
    pub fn text(&mut self, text: &str) -> &mut Self {
        let room = Self::CAPACITY - self.used;
        let take = text.len().min(room);
        self.buffer[self.used..self.used + take].copy_from_slice(&text.as_bytes()[..take]);
        self.used += take;
        self
    }

    /// Appends a hexadecimal number, prefixed.
    ///
    /// Hand-rolled because the formatting machinery allocates, and this runs where
    /// allocating may deadlock.
    pub fn hex(&mut self, value: u64) -> &mut Self {
        self.text("0x");
        let mut digits = [0_u8; 16];
        let mut count = 0;
        let mut rest = value;
        loop {
            let nibble = (rest & 0xF) as usize;
            digits[count] = b"0123456789abcdef"[nibble];
            count += 1;
            rest >>= 4;
            if rest == 0 {
                break;
            }
        }
        while count > 0 {
            count -= 1;
            if self.used < Self::CAPACITY {
                self.buffer[self.used] = digits[count];
                self.used += 1;
            }
        }
        self
    }

    /// Appends one byte as two hexadecimal digits, unprefixed - for a run of raw bytes, where a
    /// `0x` before each would be noise. Hand-rolled for the same reason [`Self::hex`] is.
    pub fn byte(&mut self, value: u8) -> &mut Self {
        if self.used + 2 <= Self::CAPACITY {
            self.buffer[self.used] = b"0123456789abcdef"[(value >> 4) as usize];
            self.buffer[self.used + 1] = b"0123456789abcdef"[(value & 0xF) as usize];
            self.used += 2;
        }
        self
    }

    /// Appends an address and, when it falls in a known region, where that is.
    pub fn address(&mut self, value: u64) -> &mut Self {
        self.hex(value);
        if let Some((name, offset)) = locate(value) {
            self.text(" (").text(name).text("+").hex(offset).text(")");
        }
        self
    }

    /// The bytes written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer[..self.used]
    }
}

/// The module being run, for the trace a fault handler writes.
static MODULE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Names the module a fault report belongs to.
pub fn describe_module(module: String) {
    let _ = MODULE.set(module);
}

/// Whether a fault has already been reported.
///
/// A faulting handler can be re-entered - the report itself may fault, or the exception
/// may be raised again as it unwinds - and a loop of half-written messages would bury
/// the one that mattered.
static REPORTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// The bytes of the faulting instruction, copied straight from the instruction pointer.
///
/// Returns a fixed buffer and how many of it are valid - no allocation, because this runs inside the
/// fault. The read is **clamped to the page `ip` sits in**: that page is mapped and readable (the
/// guest was executing in it, and execute implies read here - D065), so the read is sound; stopping
/// at the page boundary means it never reaches into a neighbour that may be unmapped, which is the
/// one way reading an instruction could fault a second time. A null or wrapped pointer yields zero
/// bytes rather than a guess.
fn instruction_bytes(ip: u64) -> ([u8; 16], usize) {
    const PAGE: u64 = 0x1000;
    let mut out = [0_u8; 16];
    if ip == 0 {
        return (out, 0);
    }
    let to_page_end = (PAGE - (ip & (PAGE - 1))) as usize;
    let len = to_page_end.min(out.len());
    // SAFETY: `ip` is the instruction pointer of the guest that just faulted, so the page holding it
    // is mapped and readable; `len` is clamped to the remainder of that one page, so the slice
    // cannot cross into an unmapped neighbour. The bytes are read once into `out` and the borrow
    // does not outlive this call.
    let src = unsafe { std::slice::from_raw_parts(ip as *const u8, len) };
    out[..len].copy_from_slice(src);
    (out, len)
}

/// The bytes immediately *before* the faulting instruction - the code that set up the registers it
/// faulted on, which a null write (`mov [rax], rax` with `rax` zero) needs to be read backwards from
/// to find where the null came from.
///
/// Clamped to the **start** of the page `ip` sits in, the mirror of [`instruction_bytes`]'s clamp to
/// the end: the whole window stays in the one mapped page, so reading it cannot fault, and a fault
/// near a page boundary simply yields fewer bytes rather than reaching into a neighbour.
fn bytes_before(ip: u64) -> ([u8; 48], usize) {
    const PAGE: u64 = 0x1000;
    let mut out = [0_u8; 48];
    if ip == 0 {
        return (out, 0);
    }
    let into_page = (ip & (PAGE - 1)) as usize;
    let len = into_page.min(out.len());
    let start = ip - len as u64;
    // SAFETY: `start .. ip` lies within the single page holding `ip`, which is mapped and readable
    // because the guest was executing in it; `len` is clamped to how far `ip` is into that page, so
    // the slice never precedes the page's start. Read once into `out`, borrow not held past the call.
    let src = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
    out[..len].copy_from_slice(src);
    (out, len)
}

/// Written explicitly so a report line is one line however the stream is buffered.
const NEWLINE: &str = "
";

/// Says, unmistakably, that the fault is in orbistoun's own code rather than the guest's.
///
/// The header names the guest's *last import call* for context, which reads as "the guest
/// faulted in this function" when it means "the emulator faulted, and this is the last thing
/// the guest asked for". That misreading cost a whole investigation once, so it is spelled out:
/// the function that actually faulted is in the host stack below, never the import above.
fn note_emulator_fault(line: &mut Line) {
    line.text("  >> EMULATOR BUG: the fault is in orbistoun's OWN code, not the guest's - the");
    line.text(NEWLINE);
    line.text(
        "     instruction pointer is outside the guest image. The import in the header is the",
    );
    line.text(NEWLINE);
    line.text(
        "     guest's last call (context only); the function that actually faulted is in the",
    );
    line.text(NEWLINE);
    line.text("     host stack below, not the 'nearest implementation' line.");
    line.text(NEWLINE);
}

/// Names a privileged or trap faulting instruction, and explains an all-ones fault address.
///
/// A guest executing `syscall`, `sysenter`, `int`, `hlt` or `ud2` has gone *under* the library
/// boundary this project intercepts (D378): there is no import to name and no library stub to
/// write - it wants kernel-level support. And the host turns the general-protection fault such
/// an instruction (or a misaligned SSE access) raises into an access violation at
/// `0xffffffffffffffff`, which looks exactly like a guest dereferencing -1 and is not a pointer
/// at all - a confusion that cost a dozen wrong eliminations before it was understood (D384).
fn note_instruction_shape(line: &mut Line, opcode: &[u8], faulting_address: u64) {
    let privileged = match opcode.first().copied() {
        Some(0xf4) => Some("hlt"),
        Some(0xcd) => Some("int (a software interrupt)"),
        Some(0x0f) if opcode.get(1) == Some(&0x05) => Some("syscall"),
        Some(0x0f) if opcode.get(1) == Some(&0x34) => Some("sysenter"),
        Some(0x0f) if opcode.get(1) == Some(&0x0b) => Some("ud2 (a deliberate trap)"),
        _ => None,
    };
    if let Some(name) = privileged {
        line.text("  >> the faulting instruction is ")
            .text(name)
            .text(NEWLINE);
        line.text(
            "     - a privileged/trap instruction orbistoun does not intercept. The guest has gone",
        );
        line.text(NEWLINE);
        line.text("     under the library boundary (D378): this wants a kernel-level handler (a syscall or");
        line.text(NEWLINE);
        line.text("     interrupt path), not a library shim, and no import is to blame.");
        line.text(NEWLINE);
    }
    if faulting_address == u64::MAX {
        line.text(
            "  >> 0xffffffffffffffff is usually a general-protection fault reported by the host",
        );
        line.text(NEWLINE);
        line.text("     (a misaligned SSE access, or a privileged instruction), not a genuine read of -1 (D384).");
        line.text(NEWLINE);
    }
}

/// Emits one fault report.
///
/// Shared by both platforms so the wording, and the region attribution, cannot drift
/// between them. `kind` carries its own preposition - "read of" wants the address
/// straight after it, "illegal instruction at" does not - so nothing is inserted here.
fn emit(kind: &str, faulting_address: u64, instruction_pointer: u64, registers: Registers) {
    use std::io::Write as _;

    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut line = Line::new();
    line.text("orbistoun: guest fault: ").text(kind).text(" ");
    line.address(faulting_address);
    line.text(" while executing at ");
    line.address(instruction_pointer);

    // Naming the import is what turns "somewhere in the emulator" into "in this function".
    // Everything up to here is allocation-free and stays that way: the label is a
    // `&'static str` from the import table, not a formatted string.
    let inside = inside_import_name(instruction_pointer);
    if let Some(name) = inside {
        line.text(" (inside ").text(name).text(")");
    }
    line.text(NEWLINE);

    // **Say loudly whose bug this is** - `inside` is set only when the instruction pointer is
    // outside the guest image, i.e. orbistoun's own code faulted, not the guest's.
    if inside.is_some() {
        note_emulator_fault(&mut line);
    }

    // **Where in *our* code, when it is our code.** `inside` names the last import called,
    // which is an attribution rather than a location: it says which function the guest
    // wanted, not which instruction faulted. For a fault in guest code that is the whole
    // story, and this project has never needed more - a fault in the emulator's own code was
    // always somebody's bug to find by reading.
    //
    // It is not enough once an implementation is complicated enough to fault inside itself.
    // The address is useless on its own because it is randomised per run, so what is printed
    // is its distance from a function in this file: add that to `emit`'s own offset in the
    // binary and the symbol is one `nm` away (D380).
    if inside.is_some()
        && let Some((name, offset)) = own_code_site(instruction_pointer)
    {
        line.text("  in orbistoun's own code, nearest implementation is ");
        line.text(name);
        line.text("+");
        line.hex(offset);
        line.text(NEWLINE);
    }

    // **The faulting instruction itself.** A fault in a wrapper-encoded guest is the one case the
    // location alone cannot be turned into an instruction by hand: the ELF `p_offset` fields do not
    // locate the loaded bytes, only the loader's wrapper decode does, so `image+0x…` names a
    // disassembly nobody can reach from the file. These bytes come straight from where the guest was
    // executing, so the report *is* the disassembler's input rather than a place to go and look
    // (D065 established this memory is readable: execute never drops read here).
    let (before, before_len) = bytes_before(instruction_pointer);
    if before_len > 0 {
        line.text("  before ");
        for &b in &before[..before_len] {
            line.byte(b).text(" ");
        }
        line.text(NEWLINE);
    }
    let (opcode, len) = instruction_bytes(instruction_pointer);
    if len > 0 {
        line.text("  bytes ");
        for &b in &opcode[..len] {
            line.byte(b).text(" ");
        }
        line.text(NEWLINE);
    }

    // Name a privileged/trap faulting instruction, and explain the all-ones address, when they
    // apply - the two most misleading fault shapes to read otherwise (D378, D384).
    note_instruction_shape(&mut line, &opcode[..len], faulting_address);

    // The stack pointer earns its place on the first line: "the guest ran out of stack"
    // and "a pointer was wrong" look identical without it, and the first is a whole class
    // of failure this project can cause by running host code on a guest stack.
    // The path through the guest's own code, which is the part an instruction pointer
    // alone cannot give. Written before the registers because it is what gets read first.
    for frame in walk_frames(registers.rbp) {
        line.text("  from ");
        line.address(frame.return_address);
        line.text(NEWLINE);
    }

    line.text("  rsp ");
    line.address(registers.rsp);
    line.text("  rdi ");
    line.hex(registers.rdi);
    line.text("  rax ");
    line.hex(registers.rax);
    line.text(NEWLINE);

    let _ = std::io::stderr().write_all(line.as_bytes());
    let _ = std::io::stderr().flush();

    // **The host stack, but only when the fault is ours.**
    //
    // The lines above are allocation-free and are out of the door before this runs, because
    // capturing a backtrace allocates, takes locks, and reads the symbol file - none of which
    // a fault handler should do before it has said the thing that matters.
    //
    // Only when the fault is in orbistoun's own code. A guest that faults on its own pointer
    // has a host stack of *this emulator's dispatch machinery*, which is noise; a guest that
    // faulted this process has one that is the whole answer, and naming the nearest
    // implementation stops short of it (D381).
    if inside.is_some() {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let mut err = std::io::stderr();
        let _ = writeln!(err, "  the host stack that got there:");
        for one in backtrace.to_string().lines().take(40) {
            let _ = writeln!(err, "  {one}");
        }
        let _ = err.flush();
    }

    // What the guest asked the kernel for directly, which a fault is very often the end of.
    // Same trade as the trace below: this allocates, and it runs after everything that must
    // not.
    syscalls_asked_for();
    paths_wanted();

    // Then the call trace, which is the part worth having. A guest that faults has
    // still said what it wanted, and losing that means the run produced nothing.
    //
    // This allocates, which the formatting above deliberately does not. The trade is
    // deliberate: a vectored handler runs in ordinary user context on a thread that
    // faulted on a *guest* pointer, so the allocator is not the thing that broke, and
    // the alternative is discarding the only output the run had. If it ever does
    // deadlock, it deadlocks a process that was about to die anyway.
    let module = MODULE.get().map_or("unknown", String::as_str);
    let (region, offset) = match locate(instruction_pointer) {
        Some((name, offset)) => (Some(name.to_owned()), Some(offset)),
        None => (None, None),
    };
    let trace = collect_with_fault(
        module,
        "Entered",
        Some(FaultSite {
            kind: kind.to_owned(),
            address: faulting_address,
            instruction_pointer,
            region,
            offset,
            inside_import: inside_import_name(instruction_pointer).map(str::to_owned),
            registers: Some(registers),
            frames: walk_frames(registers.rbp),
        }),
    );
    persist(&trace);
}

/// Says which paths the guest asked for and did not get.
///
/// **The filesystem's most useful output.** The mount table is two entries wide and what else
/// belongs in it has been an open research question; it is not one. The guest names them, and
/// a mount added afterwards then answers a request that was actually made (D387).
///
/// Read here rather than printed there, as everything else on the guest's stack is (D381).
pub(crate) fn paths_wanted() {
    use std::io::Write as _;

    let wanted = orbistoun_fs::wanted::unanswered();
    if wanted.is_empty() {
        return;
    }
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "orbistoun: the guest asked for {} path{} nothing here holds:",
        wanted.len(),
        if wanted.len() == 1 { "" } else { "s" }
    );
    for path in &wanted {
        let _ = writeln!(err, "  {path}");
        orbistoun_core::klog::note(&format!("orbistoun: no such path {path}"));
    }
    let _ = writeln!(
        err,
        "  each is a directory or file the platform has and the mount table does not - a work item, spelled by the thing that wanted it"
    );
    let _ = err.flush();
}

/// Says what the guest asked the kernel for directly.
///
/// **Read here rather than printed there.** The dispatch path runs on the guest's stack, so it
/// only sets a bit; formatting and locking a stream from that frame is what put a fault in the
/// middle of the first syscall a guest ever made here (D381).
///
/// # Why it lives in the reporting module
///
/// It was written next to the other run conditions and called from `prepare_diagnostics` -
/// which runs *before* the guest is entered, so the record it reads is always empty and the
/// report could never say anything. **A fifth setting consulted nowhere** (D082, D166, D187,
/// D379), and the one that made the syscall boundary invisible for exactly as long as it had
/// existed. Both paths a run can end by call it now: the ordinary return, and the fault.
pub(crate) fn syscalls_asked_for() {
    use std::io::Write as _;

    // The sequence first, because *when* is what says what the guest was doing (D388).
    let sequence = orbistoun_thunk::syscall::syscalls_in_order();
    if !sequence.made.is_empty() {
        let mut err = std::io::stderr();
        let _ = writeln!(
            err,
            "orbistoun: the guest made {} syscalls, in this order:",
            sequence.total
        );
        for (position, asked) in sequence.made.iter().enumerate() {
            let called = asked.name.unwrap_or("nothing here implements it");
            let _ = writeln!(
                err,
                "  {position:3}  {:5}  {called}  ({:#x})",
                asked.number, asked.argument
            );
            orbistoun_core::klog::note(&format!("orbistoun: syscall {} - {called}", asked.number));
        }
        let kept = sequence.made.len() as u64;
        if sequence.total > kept {
            let _ = writeln!(
                err,
                "  and {} more, past what this run records in order",
                sequence.total - kept
            );
        }
        let _ = err.flush();
    }

    for (number, name) in orbistoun_thunk::syscall::syscalls_asked_for() {
        let mut err = std::io::stderr();
        let _ = match name {
            Some(name) => writeln!(
                err,
                "orbistoun: the guest asked the kernel for call {number} directly, which is {name}"
            ),
            None => writeln!(
                err,
                "orbistoun: the guest asked the kernel for call {number} directly, and nothing here implements it"
            ),
        };
        let _ = err.flush();
    }
}

#[cfg(windows)]
mod imp {
    use super::emit;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    /// Let the exception carry on to whatever would otherwise have handled it.
    ///
    /// This reports and gets out of the way: swallowing the fault would leave a guest
    /// running with corrupt state, which is far worse than stopping.
    const CONTINUE_SEARCH: i32 = 0;

    /// The access violation code, which is the one that matters here.
    const ACCESS_VIOLATION: i32 = 0xC000_0005_u32 as i32;
    /// Executing something that is not code.
    const ILLEGAL_INSTRUCTION: i32 = 0xC000_001D_u32 as i32;
    /// Reaching stub padding.
    const BREAKPOINT: i32 = 0x8000_0003_u32 as i32;
    /// Running past the guard page.
    const STACK_OVERFLOW: i32 = 0xC000_00FD_u32 as i32;
    /// A debug exception - which, with a debug register armed, is a watched access.
    const SINGLE_STEP: i32 = 0x8000_0004_u32 as i32;

    /// Resume the interrupted thread with the context as it now stands.
    ///
    /// Correct here and wrong for every other exception this handler sees: a watched access
    /// has **already happened** and nothing is broken, whereas swallowing an access violation
    /// would leave a guest running on corrupt state.
    const CONTINUE_EXECUTION: i32 = -1;

    /// What the first parameter of an access violation means.
    const ACCESS_WAS_WRITE: usize = 1;
    /// A fetch from a page with no execute permission.
    const ACCESS_WAS_EXECUTE: usize = 8;

    unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // Read one field per block, as the lints require. Verbose, but each read is a
        // separate dereference of a pointer the operating system owns, and stating that
        // once per block is the discipline that keeps it checkable.

        // SAFETY: a vectored handler is passed a valid, fully populated structure that
        // stays live for the duration of the call.
        let record = unsafe { (*info).ExceptionRecord };
        // SAFETY: as above; the context record is populated for every exception.
        let context = unsafe { (*info).ContextRecord };
        // SAFETY: `record` came from the structure above and is live for this call.
        let code = unsafe { (*record).ExceptionCode };
        // SAFETY: same record. The array is fixed-size and always present.
        let parameters = unsafe { (*record).ExceptionInformation };
        // SAFETY: same record; says how many of `parameters` are meaningful.
        let count = unsafe { (*record).NumberParameters };
        // SAFETY: `context` came from the structure above and is live for this call.
        let rip = unsafe { (*context).Rip };
        // SAFETY: same context record, read once as a whole rather than field by field.
        // One dereference, one copy: `CONTEXT` is `Copy`, so this is the read the
        // per-field version was doing sixteen times.
        let ctx = unsafe { *context };

        let registers = super::Registers {
            rax: ctx.Rax,
            rbx: ctx.Rbx,
            rcx: ctx.Rcx,
            rdx: ctx.Rdx,
            rsi: ctx.Rsi,
            rdi: ctx.Rdi,
            rbp: ctx.Rbp,
            rsp: ctx.Rsp,
            r8: ctx.R8,
            r9: ctx.R9,
            r10: ctx.R10,
            r11: ctx.R11,
            r12: ctx.R12,
            r13: ctx.R13,
            r14: ctx.R14,
            r15: ctx.R15,
        };

        // Taken before anything else, because a watched access is not a failure and must not
        // be reported as one. The debug-status register says which watchpoints fired; it is
        // cleared before resuming so the next trap is not read as this one repeating.
        if code == SINGLE_STEP {
            match crate::watchpoint::note(ctx.Dr6, rip, &registers) {
                crate::watchpoint::Trap::NotOurs => return CONTINUE_SEARCH,
                crate::watchpoint::Trap::Data => {
                    // SAFETY: the context record is the one the operating system passed in,
                    // live for this call, and this writes a single field it owns before resuming.
                    unsafe {
                        (*context).Dr6 = 0;
                    }
                    return CONTINUE_EXECUTION;
                }
                crate::watchpoint::Trap::Execute { disarm } => {
                    // An execute breakpoint is one-shot: clearing its enable bit lets the
                    // instruction it sits on run now instead of trapping again, without any
                    // resume-flag or single-step dance.
                    // SAFETY: the OS's context, live for this call; a field it owns.
                    unsafe {
                        (*context).Dr6 = 0;
                    }
                    // SAFETY: as above; clears the enable bits of the fired execute slots.
                    unsafe {
                        (*context).Dr7 &= !disarm;
                    }
                    return CONTINUE_EXECUTION;
                }
            }
        }

        // A guest thread-local access whose `fs` base the host reset out from under it (D433): put
        // the base back and let the instruction run again, rather than report a fault a running
        // guest on a base-preserving host would never see. Only for access violations - the base is
        // irrelevant to an illegal instruction or a breakpoint - and only when the base has actually
        // reverted to zero, so a genuine fault still reaches the report below. Sound because a zero
        // base sends every `fs:` access into the unmapped ±2 GiB around zero, so it faults here
        // rather than reading wrong data.
        if code == ACCESS_VIOLATION && crate::tls_backstop::restore_if_reverted() {
            return CONTINUE_EXECUTION;
        }

        // An access violation reports what was attempted and where; the others carry no
        // parameters, so the faulting address is the instruction itself.
        // Taken from the two lists on `FaultSite` rather than written out again here.
        // A consumer has to be able to tell "the guest touched this address" from "this
        // is the instruction that faulted", and a second copy of these strings is a
        // second copy that drifts - after which the classification is silently wrong.
        let touched = orbistoun_report::trace::FaultSite::TOUCHED;
        let at_instruction = orbistoun_report::trace::FaultSite::AT_THE_INSTRUCTION;
        let (kind, at) = match code {
            ACCESS_VIOLATION if count >= 2 => {
                let what = match parameters[0] {
                    ACCESS_WAS_WRITE => touched[0],
                    ACCESS_WAS_EXECUTE => touched[2],
                    _ => touched[1],
                };
                (what, parameters[1] as u64)
            }
            ILLEGAL_INSTRUCTION => (at_instruction[0], rip),
            BREAKPOINT => (at_instruction[1], rip),
            STACK_OVERFLOW => (at_instruction[2], rip),
            // Anything else is not ours to explain. Debuggers and language runtimes
            // raise exceptions routinely, and reporting those as guest faults would be
            // noise at best and misleading at worst.
            _ => return CONTINUE_SEARCH,
        };

        emit(kind, at, rip, registers);
        CONTINUE_SEARCH
    }

    pub(super) fn install() -> bool {
        // SAFETY: registering a handler is safe; the function pointer has the signature
        // the platform requires and remains valid for the life of the process.
        let registered = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
        !registered.is_null()
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not implemented away from Windows yet.
    ///
    /// Reporting from inside a signal handler needs `ucontext` to reach the instruction
    /// pointer, which the crates in use here do not expose. Saying so plainly beats a
    /// handler that reports the fault address and silently omits the half that says
    /// *what was executing* - which is the more useful half (principle 3).
    pub(super) fn install() -> bool {
        false
    }
}

/// Names for stub indices, so a call trace says what the guest wanted.
///
/// Set before entering the guest. Empty is a legitimate state - a module with no
/// readable import table still runs - and the summary falls back to bare indices rather
/// than inventing names.
static IMPORT_LABELS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Where each implementation starts, sorted, so a fault inside one can be named.
///
/// **A function pointer is an address, and this project has a table of them.** The binary
/// carries no symbols on this toolchain, so an address in orbistoun's own code is opaque -
/// and once an implementation is complicated enough to fault inside itself, opaque is not
/// good enough (D380).
static OWN_CODE: std::sync::OnceLock<Vec<(u64, &'static str)>> = std::sync::OnceLock::new();

/// Publishes where each implementation starts.
///
/// Sorted here rather than at the fault, because a fault handler must not allocate or sort.
pub fn name_implementations(mut starts: Vec<(u64, &'static str)>) {
    starts.sort_unstable();
    let _ = OWN_CODE.set(starts);
}

/// The implementation an address falls in, and how far into it.
///
/// **Nearest preceding, and the distance is printed with it**, because that is what makes it
/// honest: a fault inside a library routine the compiler inlined or called - a copy, a
/// formatter - names the last implementation *before* it, which is a hint rather than a fact.
/// A small offset is a strong hint; a large one is visibly not a match.
fn own_code_site(address: u64) -> Option<(&'static str, u64)> {
    let starts = OWN_CODE.get()?;
    let index = starts
        .partition_point(|(at, _)| *at <= address)
        .checked_sub(1)?;
    let (at, name) = starts.get(index)?;
    Some((name, address - at))
}

/// Records what each stub index stands for.
///
/// Indexed by dynamic symbol index, matching the stub table exactly. An entry that is
/// empty means the symbol is not an import - most of the table - and is never called.
pub fn name_imports(labels: Vec<String>) {
    let _ = IMPORT_LABELS.set(labels);
}

/// The label for a stub index, or `None` if nothing is known about it.
pub fn label_of(index: usize) -> Option<&'static str> {
    IMPORT_LABELS
        .get()
        .and_then(|l| l.get(index))
        .filter(|l| !l.is_empty())
        .map(String::as_str)
}

/// Where the call trace is written, if anywhere.
///
/// A trace that exists only on a terminal dies with the run that produced it, and the
/// run that produced it may have taken ten minutes. Persisting it is what turns a
/// session into a work list (D077).
static TRACE_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Why the guest stopped itself, once it has.
static STOPPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// How the guest's calls measured against the calling convention.
fn abi_report() -> AbiReport {
    let seen = orbistoun_thunk::abi_conformance();
    let (sequence, import, rsp) = match seen.first_misaligned {
        Some((sequence, index, rsp)) => (
            Some(sequence),
            label_of(index as usize).map(str::to_owned),
            Some(rsp),
        ),
        None => (None, None, None),
    };
    AbiReport {
        misaligned_calls: seen.misaligned_calls,
        first_misaligned_sequence: sequence,
        first_misaligned_import: import,
        first_misaligned_rsp: rsp,
    }
}

/// How many frames the walk reports before giving up.
///
/// Bounded because the chain is guest-controlled: a corrupt or hostile frame pointer can
/// form a cycle, and a fault handler that loops is a process that dies with nothing said.
pub const MAX_FRAMES: usize = 12;

/// Walks the frame-pointer chain from `rbp`.
///
/// # Why this is worth having and why it is not always right
///
/// A fault address says *where* the guest died. It does not say **who called it**, and at
/// the top of a function - which is where a null dereference usually lands - the
/// instruction pointer alone is nearly content-free. The chain of return addresses is the
/// difference between "faulted at image+0x43c4" and a path through the guest's own code
/// (D172).
///
/// It is a best effort, and the reason is worth stating: a compiler is free to omit the
/// frame pointer, and optimised code routinely does. A walk that produces nothing is that
/// case, not a failure - which is why an empty list is reported rather than an error.
///
/// **Every read is bounds-checked against the stack region** before it happens. This runs
/// inside a fault handler, on a thread that has already faulted once; a second fault here
/// would replace the report with silence.
fn walk_frames(rbp: u64) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut current = rbp;

    for _ in 0..MAX_FRAMES {
        // Both the saved pointer and the return address must lie inside the stack, and a
        // frame must be aligned - anything else is a chain that has left the rails.
        if current == 0 || current % 8 != 0 || locate(current).is_none_or(|(r, _)| r != "stack") {
            break;
        }
        let Ok(at) = usize::try_from(current) else {
            break;
        };
        // SAFETY: `current` was just confirmed to lie inside the guest stack region this
        // process reserved and still holds, and to be eight-byte aligned. This is the
        // saved frame pointer a prologue pushed.
        let saved = unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at)) };
        // SAFETY: the word above it, which is the return address the call placed there.
        // Inside the same validated frame, one word further up.
        let return_address =
            unsafe { std::ptr::read(std::ptr::with_exposed_provenance::<u64>(at + 8)) };
        if return_address == 0 {
            break;
        }
        frames.push(Frame {
            return_address,
            frame_pointer: current,
        });
        // A chain must climb. Anything else is a cycle, and a fault handler that loops
        // dies with nothing said.
        if saved <= current {
            break;
        }
        current = saved;
    }
    frames
}

/// Names the import the guest was inside, when the fault was not in guest code.
///
/// An instruction pointer inside a region orbistoun placed is the guest running its own
/// code, and the import that got it there is history rather than cause. Outside every
/// placed region it is *our* code running on the guest's behalf - and then the last
/// import the guest entered is almost certainly the one that faulted.
///
/// Allocation-free: the label is a `&'static str` out of the import table, because this
/// runs before the part of the report that is allowed to allocate.
fn inside_import_name(instruction_pointer: u64) -> Option<&'static str> {
    if locate(instruction_pointer).is_some() {
        return None;
    }
    label_of(orbistoun_thunk::last_call()?.index as usize)
}

/// Records where to persist the trace, and which module it belongs to.
pub fn trace_to(path: std::path::PathBuf) {
    let _ = TRACE_PATH.set(path);
}

/// Every system call this run was asked for by number, with what is known about each.
///
/// The first argument comes from the ordered log rather than the seen-bitmap, because the
/// bitmap records only *that* a number was asked for. For a call nobody can name, that
/// argument is most of what there is to go on.
fn asked_syscalls() -> Vec<orbistoun_report::trace::AskedSyscall> {
    let ordered = orbistoun_thunk::syscall::syscalls_in_order();
    orbistoun_thunk::syscall::syscalls_asked_for()
        .into_iter()
        .map(|(number, name)| orbistoun_report::trace::AskedSyscall {
            number,
            name: name.map(str::to_owned),
            first_argument: ordered
                .made
                .iter()
                .find(|asked| asked.number == number)
                .map(|asked| asked.argument),
        })
        .collect()
}

/// Collects what the guest called, most-used first.
///
/// Shared by the time-limit path and the ordinary-return path, so a guest that stops on
/// its own is reported exactly as fully as one that had to be stopped.
pub fn collect_calls(module: &str, reached: &str) -> CallTrace {
    collect_with_fault(module, reached, None)
}

/// Collects the trace, recording where the guest died.
pub fn collect_with_fault(module: &str, reached: &str, fault: Option<FaultSite>) -> CallTrace {
    // What the watched region became, printed here because this is the one point reached
    // on **both** endings - a fault and a time limit. A diagnostic that only reported when
    // the guest crashed would be silent for the title that never crashes (D223).
    let changed = crate::watch::changes();
    if !changed.is_empty() {
        use std::io::Write as _;
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "orbistoun: watched region:");
        for line in &changed {
            let _ = writeln!(err, "{line}");
        }
    }

    let mut counts: Vec<(usize, u64)> = orbistoun_thunk::call_counts()
        .into_iter()
        .enumerate()
        .filter(|(_, n)| *n > 0)
        .collect();
    counts.sort_unstable_by_key(|(_, calls)| std::cmp::Reverse(*calls));

    CallTrace {
        module: module.to_owned(),
        reached: reached.to_owned(),
        // Summed from the same snapshot the rows are drawn from, rather than read from
        // the global counter. The guest may still be running, so a separately-read total
        // disagrees with the rows beneath it - and a report that does not add up invites
        // the reader to distrust all of it.
        total_calls: counts.iter().map(|(_, n)| *n).sum(),
        distinct: counts.len(),
        // **Recorded, not just printed.** These used to reach stderr at the end of a run and
        // go no further, so a guest that talks to the kernel by number left nothing behind for
        // the work list to rank - and that is how every open-toolchain payload works (D401).
        syscalls: asked_syscalls(),
        tail: tail_of_recorded(),
        formats: {
            let seen = orbistoun_libc::format_stats();
            FormatReport {
                calls: seen.calls,
                refused: seen.refused,
                truncated: seen.truncated,
                first_fault: seen
                    .first_fault
                    .map(describe_format_fault)
                    .unwrap_or_default(),
            }
        },
        dumps: collected_dumps(),
        conditions: {
            // Merged at read time, because the conditions are recorded before the import
            // labels exist and resolving which import to plant into needs them (D218).
            let mut c = CONDITIONS.get().cloned().unwrap_or_default();
            if let Some(planted) = PLANTED.get() {
                // **With the counts, always.** A forced write that matched an import and
                // then refused every target is indistinguishable from one that landed and
                // changed nothing - and the second is a result while the first is a broken
                // experiment. Reporting the number is what tells them apart (D218).
                let (done, refused) = orbistoun_thunk::forced_write_counts();
                // The same argument for forced returns, which had the same hole: an
                // unmoved fault under a forced answer reads as "the return value is not
                // where that came from", and reads identically when nothing was ever
                // answered. Counted separately rather than folded into the plant counts,
                // because they are different mechanisms and a reader adding them together
                // would learn nothing (D230).
                let answered = orbistoun_thunk::forced_return_count();
                let mut counts = Vec::new();
                if done != 0 || refused != 0 {
                    counts.push(format!("{done} planted, {refused} refused"));
                }
                if answered != 0 {
                    counts.push(format!("{answered} answered"));
                }
                c.experiments = if counts.is_empty() {
                    planted.clone()
                } else {
                    format!("{planted} ({})", counts.join("; "))
                };

                // Asked for and applied zero times. Recorded rather than left to be
                // inferred from a count of nought buried in a conditions line, because
                // that is exactly what was there before and it was read straight past
                // (D241).
                if planted.contains("at *arg") && done == 0 {
                    c.did_nothing
                        .push("ORBISTOUN_WRITE planted nothing".to_owned());
                }
                if planted.contains("answers") && answered == 0 {
                    c.did_nothing
                        .push("ORBISTOUN_RETURN answered no call".to_owned());
                }
            }
            c
        },
        stopped: STOPPED.get().cloned(),
        abi: abi_report(),
        reads: {
            let seen = orbistoun_fs::open::read_stats();
            ReadReport {
                reads: seen.reads,
                short: seen.short,
                bytes: seen.bytes,
            }
        },
        calls: counts
            .iter()
            .map(|(index, calls)| CalledImport {
                index: *index,
                label: labelled(*index),
                calls: *calls,
                implemented: orbistoun_thunk::is_implemented(*index),
            })
            .collect(),
        fault,
    }
}

/// What to call a stub in a report.
///
/// **Names the slot when it cannot name the function.** A bare `unknown` is the one label
/// that cannot be looked into: two different unlabelled stubs read as the same thing, so a
/// trace showing both looks like one function called twice. The index is always available
/// and always distinguishes them (D366).
fn labelled(index: usize) -> String {
    label_of(index).map_or_else(|| format!("unknown#{index}"), str::to_owned)
}

/// The last calls the guest made, labelled.
///
/// Taken from the ring the dispatcher has always filled. It holds the *first*
/// [`orbistoun_thunk::MAX_RECORDED_CALLS`], so for any run that stays under that - which
/// every run so far does, by an order of magnitude - the end of it is the true end.
fn tail_of_recorded() -> Vec<TracedCall> {
    let all = orbistoun_thunk::recorded_calls();
    let from = all.len().saturating_sub(TAIL_CALLS);
    all[from..]
        .iter()
        .map(|c| TracedCall {
            sequence: c.sequence,
            label: labelled(c.index as usize),
            arg0: c.arg0,
            from: c.from,
            returned: c.ret,
        })
        .collect()
}

/// Writes a trace to wherever [`trace_to`] pointed, if anywhere.
///
/// The findings are derived on read rather than stored: they are a *conclusion* about a
/// trace, and a stored conclusion goes stale the moment the rules that drew it improve.
///
/// Failures are reported and swallowed: losing a trace is bad, and failing a run that
/// otherwise worked because a directory was missing is worse.
pub fn persist(trace: &CallTrace) {
    // Every path that ends a run comes through here, so this is the one place a watchpoint
    // summary is guaranteed to be reached - including a fault, which is the path it was
    // built for. Silent unless something was armed.
    crate::watchpoint::summarise();
    // Same argument, same place: this is the one path every ending run comes through, so
    // it is the only place a shell summary is guaranteed to be reached - including after a
    // fault, which is when somebody most wants to know what the guest was and was not told.
    crate::session::summarise();
    let Some(path) = TRACE_PATH.get() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(trace) {
        Ok(text) => {
            if let Err(e) = std::fs::write(path, text) {
                eprintln!(
                    "orbistoun: could not write the call trace to {}: {e}",
                    path.display()
                );
            }
        }
        Err(e) => eprintln!("orbistoun: could not serialise the call trace: {e}"),
    }
}

/// Exit status used when a guest outruns its time limit.
///
/// Deliberately not in the range the platform uses for faults, so the two can never be
/// confused: "still running when the clock ran out" and "died on a bad pointer" call for
/// completely different next steps.
pub const TIME_LIMIT_EXIT: i32 = 0x0B0E;

/// Exit status for a guest stopped by the call budget.
///
/// Distinct from [`TIME_LIMIT_EXIT`] on purpose. "Ran out of clock" and "made the number of
/// calls it was allowed" look identical in a summary and mean different things: the first
/// varies with the machine and the second does not (D238).
pub const CALL_BUDGET_EXIT: i32 = 0x0B0F;

/// Ends the process if the guest is still running after `seconds`.
///
/// Records that the guest stopped itself, and ends the process.
///
/// Installed as the handler the subsystem crates call, because they cannot call upwards -
/// this is the layer that knows a trace is being written and where it goes.
///
/// **The trace is persisted before exiting**, for the same reason the time limit persists
/// before it kills: a guest that gave up has still said what it wanted, and that is the
/// whole output of the run.
fn guest_stopped(reason: orbistoun_core::StopReason, code: u64) -> ! {
    use std::io::Write as _;

    // Recorded before the trace is collected, so the report says the guest stopped rather
    // than describing an absent fault as a time limit.
    let _ = STOPPED.set(reason.label().to_owned());
    // **The third way a run ends, and the reports were on the other two.** A server does not
    // fault and does not return: it runs until the time limit, which is how `klogsrv`,
    // `shsrv` and `zftpd` all stop. So the two reports that read a record after the guest has
    // stopped were installed on the fault path and the ordinary return, and never fired for
    // any of the payloads that actually work (D387).
    syscalls_asked_for();
    paths_wanted();
    let module = MODULE.get().map_or("unknown", String::as_str);
    let trace = collect_calls(module, "Entered");
    persist(&trace);

    let mut line = Line::new();
    line.text("orbistoun: ").text(reason.label()).text(" (");
    line.hex(code);
    line.text(
        ")
",
    );
    let _ = std::io::stderr().write_all(line.as_bytes());
    let _ = std::io::stderr().flush();

    std::process::exit(orbistoun_core::stop::EXIT_GUEST_STOPPED);
}

/// Reports what the guest managed first. A guest that hangs and is killed from outside
/// takes its call trace with it, which is the one thing the run was for - so the trace
/// is written from in here, where it is still reachable.
///
/// Runs on an ordinary thread rather than in a fault handler, so it may allocate.
pub fn install_stop_handler() {
    orbistoun_core::stop::on_guest_stop(guest_stopped);
}

/// What the guest was pointing at, described rather than dumped raw.
///
/// The address is named against a region because `stack+0x800c90` says something a bare
/// `0x600000800c90` does not - it says the guest handed over a local, which is what
/// distinguishes an out-parameter from a pointer into its own data.
fn collected_dumps() -> Vec<ArgumentDump> {
    orbistoun_thunk::argument_dumps()
        .into_iter()
        .map(|d| ArgumentDump {
            label: label_of(d.index as usize).unwrap_or("unknown").to_owned(),
            slot: d.slot,
            value: d.address,
            // Named against a region only when it pointed at one. A scalar has no region,
            // and inventing one for it would read as though it were an address (D198).
            //
            // An address-shaped value that no region covers gets said out loud instead of
            // rendering as a bare number, because those two look identical and mean
            // opposite things - one is a count, the other is a pointer that is wrong or a
            // region this run never declared (D217).
            at: match d.pointing {
                orbistoun_thunk::Pointing::Mapped => locate(d.address).map_or_else(
                    || format!("{:#x}", d.address),
                    |(r, o)| format!("{r}+{o:#x}"),
                ),
                orbistoun_thunk::Pointing::Unreadable => {
                    "no region this run mapped, and address-shaped".to_owned()
                }
                orbistoun_thunk::Pointing::Scalar => String::new(),
            },
            bytes: if d.pointing.was_read() {
                d.bytes
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                String::new()
            },
            text: if d.pointing.was_read() {
                printable(&d.bytes)
            } else {
                String::new()
            },
        })
        .collect()
}

/// The bytes as text, when they read as text.
///
/// Returned empty rather than as escapes when they do not: a line of dots is noise, and
/// noise beside real hex makes the hex harder to read. A name or a path in here is often
/// the entire answer.
fn printable(bytes: &[u8]) -> String {
    let text: String = bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| char::from(*b))
        .collect();
    if text.len() >= 3 && text.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        text
    } else {
        String::new()
    }
}

/// Says what a formatted write could not do, in words a report can print.
///
/// Named here rather than in the crate that produces it: the distinction a reader needs is
/// between "implement this" and "the value never arrived", and only a sentence carries it.
fn describe_format_fault(fault: orbistoun_libc::FormatFault) -> String {
    match fault {
        orbistoun_libc::FormatFault::Unsupported(c) => {
            format!("the %{c} conversion is not implemented")
        }
        orbistoun_libc::FormatFault::FloatingPoint(c) => format!(
            "%{c} takes its argument in a vector register, which the trampoline does not capture"
        ),
        orbistoun_libc::FormatFault::OutOfArguments => {
            "the format needed more arguments than arrive in registers".to_owned()
        }
    }
}

/// Records what this run is subject to, for every trace it produces.
///
/// Set once during setup by the code that knows the limit and the policy, which is the
/// same dependency inversion the stop handler uses: a trace is collected from a fault
/// handler that has no route back up to the configuration (D160).
pub fn record_conditions(conditions: Conditions) {
    let _ = CONDITIONS.set(conditions);
}

/// What the run is subject to. Empty until setup records it.
static CONDITIONS: std::sync::OnceLock<Conditions> = std::sync::OnceLock::new();

/// Every diagnostic in force, if any.
///
/// A second slot rather than a field set with the rest: the conditions are recorded before
/// the import labels exist, and deciding which import an experiment applies to needs them.
/// Merged when the trace is collected.
static PLANTED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Records what the run was put under, for the run conditions.
pub fn note_experiments(what: String) {
    let _ = PLANTED.set(what);
}

/// Reports what the guest managed first, if it runs out of time.
pub fn start_time_limit(seconds: u64, module: String) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        let trace = collect_calls(&module, "Entered");
        // Persisted *before* the summary is printed and before the process ends. A
        // guest that had to be stopped is exactly the case where the trace matters
        // most, and exactly the case where nothing else will get a chance to save it.
        persist(&trace);
        summarise_calls(&format!("after {seconds}s"), &trace);
        std::process::exit(TIME_LIMIT_EXIT);
    });
}

/// The module a budgeted run is executing, for the stop callback to name.
static BUDGET_MODULE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// The budget in force, so the summary can say what was reached.
static BUDGET_CALLS: AtomicU64 = AtomicU64::new(0);

/// Stops the guest once it has made `budget` import calls.
///
/// **The deterministic half of the pair.** The wall-clock limit fixes the duration and lets
/// the call count vary, which is backwards: the count is what every verdict is read off,
/// and it moved 13% between identical runs. This fixes the count instead (D238).
///
/// Not a replacement for the clock. A guest that stops calling imports never reaches a
/// budget, so both are installed and either may fire.
pub fn start_call_budget(budget: u64, module: String) {
    let _ = BUDGET_MODULE.set(module);
    BUDGET_CALLS.store(budget, Ordering::Relaxed);
    orbistoun_thunk::install_call_budget(budget, on_budget_reached);
}

/// Writes the trace and ends the process, when the budget is reached.
///
/// A plain `fn` because it is installed as one: the dispatch path holds a function pointer
/// and cannot hold a closure without an allocation the call path must not make
/// (principle 9). What it would have captured lives in the two statics above.
fn on_budget_reached() {
    let module = BUDGET_MODULE.get().map_or("", String::as_str);
    let trace = collect_calls(module, "Entered");
    persist(&trace);
    // Not "after N calls": the count is already the second half of that line, and saying
    // it twice reads as two different numbers that happen to agree.
    summarise_calls("when its call budget ran out", &trace);
    std::process::exit(CALL_BUDGET_EXIT);
}

/// Writes what the guest called, most-used first.
///
/// The counts are the work list: implementing the top of this list is what moves a
/// guest further, and the order is not guessable from a static import dump.
fn summarise_calls(stopped: &str, trace: &CallTrace) {
    use std::io::Write as _;

    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "orbistoun: the guest was still running {stopped}; {} import calls across {} distinct imports",
        trace.total_calls, trace.distinct
    );
    // A diagnostic that intervened says what it did. Silence here means none was asked
    // for; a line reporting nothing fired means the run tested nothing, which is the one
    // conclusion that must never be mistaken for an elimination (D325).
    for fill in [
        orbistoun_kernel::direct_fill_summary(),
        orbistoun_libc::heap_fill_summary(),
    ]
    .into_iter()
    .flatten()
    {
        let _ = writeln!(err, "orbistoun: {fill}");
    }
    for call in trace.calls.iter().take(MOST_CALLED_REPORTED) {
        // Integer tenths of a percent rather than floating point: the counts run into
        // the hundreds of millions, past the point where an `f64` holds them exactly,
        // and a share that does not quite add up invites distrust of the whole report.
        let tenths = call
            .calls
            .saturating_mul(1000)
            .checked_div(trace.total_calls)
            .unwrap_or(0);
        // Formatted as text, so the width below is a width. A precision on a string
        // truncates it - `{share:5.1}` silently turned "99.9" into "9".
        let share = format!("{}.{}", tenths / 10, tenths % 10);
        let _ = writeln!(
            err,
            "orbistoun:   {:>12} calls ({share:>5}%)  {}",
            call.calls, call.label
        );
    }
    let _ = err.flush();
}

/// How many of the most-called imports are listed when a limit expires.
const MOST_CALLED_REPORTED: usize = 20;

/// Installs the fault reporter, returning whether one is active.
///
/// `false` is not an error: it means this platform reports nothing beyond the exit
/// status, and a caller should say so rather than imply a fault report is coming.
pub fn install() -> bool {
    imp::install()
}

#[cfg(test)]
mod tests {
    use super::{Line, Region, describe_region, locate};

    #[test]
    fn an_address_inside_a_registered_region_is_named_with_its_offset() {
        // The difference between one bit of information and a work list.
        describe_region(Region::Image, 0x4000_0000_0000, 0x10_0000);
        assert_eq!(locate(0x4000_0000_1234), Some(("image", 0x1234)));
    }

    #[test]
    fn an_address_outside_every_region_is_left_unnamed_rather_than_guessed() {
        // Attributing an unmapped address to the nearest region would point a reader
        // at code that had nothing to do with it.
        describe_region(Region::Stubs, 0x7000_0000_0000, 0x1000);
        assert_eq!(locate(0x1234), None);
    }

    #[test]
    fn the_end_of_a_region_is_outside_it() {
        // Off-by-one here would name the first byte of whatever follows.
        describe_region(Region::Stack, 0x6000_0000_0000, 0x1000);
        assert_eq!(locate(0x6000_0000_0FFF), Some(("stack", 0xFFF)));
        assert_eq!(locate(0x6000_0000_1000), None);
    }

    #[test]
    fn hexadecimal_is_formatted_without_allocating() {
        // Hand-rolled because a handler may run while the allocator lock is held by the
        // code that just crashed.
        let mut line = Line::new();
        line.hex(0);
        assert_eq!(line.as_bytes(), b"0x0");

        let mut line = Line::new();
        line.hex(0xDEAD_BEEF);
        assert_eq!(line.as_bytes(), b"0xdeadbeef");

        let mut line = Line::new();
        line.hex(u64::MAX);
        assert_eq!(line.as_bytes(), b"0xffffffffffffffff");
    }

    #[test]
    fn a_line_truncates_rather_than_overflowing_its_buffer() {
        // The buffer is fixed and the handler cannot grow it; losing the tail of a
        // message is survivable, writing past it is not.
        let mut line = Line::new();
        for _ in 0..200 {
            line.text("some text that is long enough to overrun");
        }
        assert_eq!(line.as_bytes().len(), Line::CAPACITY);
    }
}
