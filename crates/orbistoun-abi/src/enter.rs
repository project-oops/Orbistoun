//! Entering the guest: the host-to-guest direction of the same boundary.
//!
//! Guest code runs on its own stack, so an overrun hits a guard page instead of corrupting host
//! frames (D063). The guest follows System V and may destroy registers Windows expects to survive
//! (`rsi`, `rdi`, `xmm6`-`xmm15`), so every System V caller-saved register is declared clobbered
//! and the compiler saves what it needs. `r12`, callee-saved under System V, holds the host stack
//! pointer across the call. The process stack image is built in `orbistoun-loader::process`;
//! this crate only transfers control. It also holds the diagnostic argument blocks and gadget
//! stubs that make a guest's reads and calls legible.

/// Transfers control to guest code on a dedicated stack.
///
/// Returns whatever the guest leaves in `rax`, if it returns at all; most fault or take a path
/// that never comes back, which is why this runs in a worker process.
///
/// # Safety
///
/// Executes arbitrary guest machine code, which the compiler cannot check. The caller must
/// ensure:
///
/// - `entry` points at mapped, executable, fully relocated guest code.
/// - `stack_pointer` is the top of a mapped, writable, guest-owned stack, aligned to
///   sixteen bytes, with room beneath it for the guest to grow into.
/// - No host state that must survive is reachable only through registers the guest may
///   destroy.
pub unsafe fn enter_guest(entry: u64, stack_pointer: u64) -> u64 {
    // SAFETY: the caller's guarantees are exactly the ones the general form needs, and the
    // argument is a block this crate owns for as long as the process runs.
    unsafe { enter_guest_with_argument(entry, stack_pointer, process_argument_block()) }
}

/// The float environment a title runs under on the hardware, as configuration bits.
///
/// A conformance run read `MXCSR` as `0x9fe0` on entry to a native title: flush-to-zero,
/// round-to-nearest, all exceptions masked, denormals-are-zero, and the precision status flag.
/// Status bits record float work already done before the title started, so this is `0x9fc0`,
/// the same configuration with status clear. The host default has DAZ and FTZ clear, which
/// would make denormal arithmetic quietly differ from the hardware.
pub const GUEST_MXCSR: u32 = 0x9fc0;

/// Puts this thread into the float environment a title runs under.
///
/// Per thread, so every path into guest code calls it: the process entry and each guest thread.
/// It also covers orbistoun's own implementations on that thread, which stand in for the
/// platform's C library running under the same environment.
#[cfg(target_arch = "x86_64")]
pub fn adopt_guest_float_environment() {
    let value = GUEST_MXCSR;
    // SAFETY: `ldmxcsr` reads four bytes from the operand, and `value` is a live `u32` on this
    // frame. It writes no memory and touches no register the compiler is tracking.
    unsafe {
        core::arch::asm!(
            "ldmxcsr [{control}]",
            control = in(reg) &raw const value,
            options(nostack, readonly, preserves_flags),
        );
    }
}

/// Puts this thread into the float environment a title runs under.
#[cfg(not(target_arch = "x86_64"))]
pub fn adopt_guest_float_environment() {}

/// This thread's current `MXCSR`, so the setting above can be read back.
#[cfg(target_arch = "x86_64")]
#[must_use]
pub fn float_environment() -> u32 {
    let mut value = 0_u32;
    // SAFETY: `stmxcsr` writes four bytes to the operand, and `value` is a live `u32` on this
    // frame with nothing else aliasing it.
    unsafe {
        core::arch::asm!(
            "stmxcsr [{control}]",
            control = in(reg) &raw mut value,
            options(nostack, preserves_flags),
        );
    }
    value
}

/// This thread's current `MXCSR`, where there is one.
#[cfg(not(target_arch = "x86_64"))]
#[must_use]
pub fn float_environment() -> u32 {
    0
}

/// Transfers control to a process entry point, and never comes back.
///
/// System V puts `rsp` sixteen-byte aligned at the entry point, pointing at the argument count;
/// a `call` would push a return address there and misalign the stack, so a process is jumped
/// to. A program leaves by calling exit; an entry point executing `ret` jumps to its argument
/// count and faults on a small address. The fault handler and the time limit persist the trace
/// from the guest's thread (D238). `rbp` is zeroed to end the frame chain.
///
/// # Safety
///
/// Executes arbitrary guest machine code and never returns. `entry` must point at mapped,
/// executable, fully relocated code; `stack_pointer` must be the sixteen-byte-aligned
/// address of a written process image inside a mapped, writable guest stack. `argument`
/// is passed unexamined, so if the guest dereferences it, it must be valid.
#[cfg(target_arch = "x86_64")]
pub unsafe fn enter_process(entry: u64, stack_pointer: u64, argument: u64) -> ! {
    // SAFETY: the caller guarantees the entry, the stack and the argument. Nothing after this runs
    // on this thread, so no host state survives and no clobber list is needed; `noreturn` is
    // correct. The entry and argument are pinned to explicit registers so the argument move into
    // `rdi` cannot destroy the entry before the jump.
    unsafe {
        core::arch::asm!(
            "mov rsp, {stack}",
            "xor ebp, ebp",
            "mov rdi, r11",
            "jmp r10",
            stack = in(reg) stack_pointer,
            in("r10") entry,
            in("r11") argument,
            options(noreturn),
        );
    }
}

/// Transfers control to a process entry point, and never comes back.
///
/// # Safety
///
/// See the x86-64 documentation; this build cannot do it at all.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn enter_process(_entry: u64, _stack_pointer: u64, _argument: u64) -> ! {
    unimplemented!("entering a guest process is x86-64 only")
}

/// Whether this build can execute guest code at all.
///
/// An architecture check: guest code is x86-64 and runs natively, with no execution backend
/// (D031). A build for another architecture analyses containers and traces but cannot run a
/// guest, and this lets that limit be reported rather than hit as a panic.
#[must_use]
pub const fn can_execute_guests() -> bool {
    cfg!(target_arch = "x86_64")
}

/// How much zeroed memory the process argument block holds: a page, so any offset the entry
/// point reads is inside it.
pub const ARGUMENT_BLOCK_SIZE: usize = 4096;

/// A zeroed block for the entry point to read its process arguments out of.
///
/// A process entry point dereferences the pointer in its first argument register immediately,
/// so it is given a defined block rather than whatever the host left in `rdi`. Zeroed and never
/// written, because the real layout is not known from any lawful source: a count reads as none,
/// a pointer as null.
pub fn process_argument_block() -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    // A host heap address, so not deterministic across runs: this leaf crate has no orbistoun
    // dependency through which to reach `orbistoun_mem::blocks`.
    *BLOCK.get_or_init(|| {
        let block: Box<[u64; ARGUMENT_BLOCK_SIZE / 8]> = Box::new([0; ARGUMENT_BLOCK_SIZE / 8]);
        std::ptr::from_mut(Box::leak(block)) as usize as u64
    })
}

/// Where a sentinel block's markers start: canonical, unmapped, and recognisable on sight in a
/// fault report.
pub const SENTINEL_BASE: u64 = 0x0000_5E27_0000_0000;

/// How far apart consecutive markers sit.
///
/// Wide, so a small displacement the guest adds (`call [rax+8]`) stays inside the slot, and the
/// fault names both the marker and what was added.
pub const SENTINEL_STRIDE: u64 = 0x1000;

/// A block whose every slot holds a distinct, identifiable unmapped marker.
///
/// The zeroed block says the guest wanted a pointer; this says which field, because the faulting
/// address names the slot, identifying a field in one boot (D286). A diagnostic, not a claim
/// about the layout, and `orbistoun-env` records that a run used it.
pub fn sentinel_argument_block() -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        let mut block: Box<[u64; ARGUMENT_BLOCK_SIZE / 8]> = Box::new([0; ARGUMENT_BLOCK_SIZE / 8]);
        for (slot, cell) in block.iter_mut().enumerate() {
            *cell = SENTINEL_BASE + (slot as u64) * SENTINEL_STRIDE;
        }
        std::ptr::from_mut(Box::leak(block)) as usize as u64
    })
}

/// A block whose every slot points at a function that returns zero.
///
/// Asks how far the guest gets if every field it calls answers harmlessly: a guest that goes on
/// to its imports treats the structure as a table of functions, and one that faults somewhere
/// new names a field that needed to be data. A diagnostic, not a claim about the layout.
pub fn answering_argument_block() -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        // A whole page of `ret`, so any entry point into it returns; a short stub in a zeroed page
        // would run off its end into `00 00`, an `add [rax], al`.
        let mut code = vec![0xC3_u8; ARGUMENT_BLOCK_SIZE];
        // And at the front, answer zero rather than whatever was left in `rax`.
        code[..3].copy_from_slice(&[0x31, 0xC0, 0xC3]);
        // Leaked: the guest holds these addresses for as long as it runs.
        let stub = Box::leak(Box::new(
            crate::exec::ExecutableBuffer::new(&code).expect("a page of stub must be mappable"),
        ));
        // Two kinds of field, two answers. Slot zero, which every measured payload calls first, gets the
        // returning page; every other slot gets its own writable page, so a write succeeds while a call
        // faults on an instruction fetch at an address that names the slot.
        let slots = ARGUMENT_BLOCK_SIZE / 8;
        let arena: Box<[[u8; ARGUMENT_BLOCK_SIZE]]> =
            vec![[0_u8; ARGUMENT_BLOCK_SIZE]; slots].into_boxed_slice();
        let arena = Box::leak(arena);
        let arena_base = arena.as_mut_ptr() as usize as u64;

        let mut block: Box<[u64; ARGUMENT_BLOCK_SIZE / 8]> = Box::new([0; ARGUMENT_BLOCK_SIZE / 8]);
        for (slot, cell) in block.iter_mut().enumerate() {
            *cell = if slot == 0 {
                stub.address()
            } else {
                arena_base + (slot as u64) * ARGUMENT_BLOCK_SIZE as u64
            };
        }
        std::ptr::from_mut(Box::leak(block)) as usize as u64
    })
}

/// What a reporting stub says when the guest calls one.
///
/// Printed and flushed per line, because the guest's next act is usually the fault this
/// explains.
extern "sysv64" fn report_call(slot: u64, first: u64, second: u64, third: u64) -> u64 {
    use std::io::Write as _;

    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "orbistoun: the guest called handoff slot {slot} with ({first:#x}, {second:#x}, {third:#x})"
    );
    let _ = err.flush();
    0
}

/// A block whose every slot points at a function that says how it was called.
///
/// Answers harmlessly and reports the slot and the first three arguments, so a field called with
/// a string pointer names itself as a resolver (D365). One emitted stub per slot shifts the
/// guest's arguments one register along and puts its slot index first; arguments beyond the
/// third are dropped. A diagnostic, not a claim about the layout.
pub fn reporting_argument_block() -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        let mut block: Box<[u64; ARGUMENT_BLOCK_SIZE / 8]> = Box::new([0; ARGUMENT_BLOCK_SIZE / 8]);
        for (slot, cell) in block.iter_mut().enumerate() {
            // Leaked: the guest holds the address for as long as it runs.
            let stub = Box::leak(Box::new(
                crate::exec::ExecutableBuffer::new(&reporting_stub(slot as u64))
                    .expect("a reporting stub must be mappable"),
            ));
            *cell = stub.address();
        }
        std::ptr::from_mut(Box::leak(block)) as usize as u64
    })
}

/// The structure a payload's runtime is handed, as far as it is known.
///
/// Field zero is a function the guest calls with a module number, a string and an out pointer;
/// the string is `sceKernelDlsym`, so field zero is the resolver (D365). The rest hold
/// [`UnknownFields`]. `unknown_base` may name a region the caller mapped and zeroed, so a field
/// read as a pointer yields zero and the runtime carries on while its address still names the
/// field; [`SENTINEL_BASE`] with nothing mapped stops at the first use.
pub fn handoff_argument_block(resolver: u64, unknown: UnknownFields, named: &[[u64; 2]]) -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        let mut block: Box<[u64; ARGUMENT_BLOCK_SIZE / 8]> = Box::new([0; ARGUMENT_BLOCK_SIZE / 8]);
        for (slot, cell) in block.iter_mut().enumerate() {
            *cell = if slot == 0 {
                resolver
            } else if (1..=5).contains(&slot) {
                match unknown {
                    UnknownFields::Markers { base } => base + (slot as u64) * SENTINEL_STRIDE,
                    UnknownFields::Zero => 0,
                }
            } else {
                0
            };
        }
        // Last, so a named field wins, including over the resolver in field zero.
        for [field, value] in named {
            if let Ok(slot) = usize::try_from(*field)
                && let Some(cell) = block.get_mut(slot)
            {
                *cell = *value;
            }
        }
        std::ptr::from_mut(Box::leak(block)) as usize as u64
    })
}

/// What the handoff structure's unestablished fields hold.
///
/// A marker names the field the guest used; zero names nothing but is a value a correct program
/// checks, so a runtime reading a field it does not need carries on. Neither is a claim about
/// the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownFields {
    /// Each field names itself, from a base the caller may have mapped.
    Markers {
        /// Where the marker range starts - [`SENTINEL_BASE`] unless the caller mapped its own.
        base: u64,
    },
    /// Every field reads as zero.
    Zero,
}

/// The bytes of one reporting stub, split out so the encoding is testable without mapping.
fn reporting_stub(slot: u64) -> Vec<u8> {
    let reporter: extern "sysv64" fn(u64, u64, u64, u64) -> u64 = report_call;
    naming_stub(slot, reporter as *const () as usize as u64)
}

/// Bytes of one stub: what the six instructions below need.
pub const STUB_LEN: usize = 32;

/// A stub that shifts the guest's arguments along and puts `names` in the first.
///
/// ```text
/// mov rcx, rdx     the guest's third argument moves to the fourth
/// mov rdx, rsi     its second to the third
/// mov rsi, rdi     its first to the second
/// mov rdi, imm64   the value that says which stub this is
/// mov r11, imm64   the reporter
/// jmp r11
/// ```
///
/// `r11` is caller-saved and carries no argument. The jump is a tail call, so the reporter
/// returns straight to the guest.
fn naming_stub(names: u64, reporter: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(STUB_LEN);
    code.extend_from_slice(&[0x48, 0x89, 0xD1]);
    code.extend_from_slice(&[0x48, 0x89, 0xF2]);
    code.extend_from_slice(&[0x48, 0x89, 0xFE]);
    code.extend_from_slice(&[0x48, 0xBF]);
    code.extend_from_slice(&names.to_le_bytes());
    code.extend_from_slice(&[0x49, 0xBB]);
    code.extend_from_slice(&reporter.to_le_bytes());
    code.extend_from_slice(&[0x41, 0xFF, 0xE3]);
    code
}

/// Fields whose contents get stubs rather than markers; fields past these still name themselves
/// as markers.
pub const STUBBED_FIELDS: u64 = 16;

/// Members per field that get one: sixty-four eight-byte members, half a kilobyte per structure.
pub const STUBBED_MEMBERS: u64 = 64;

/// What a member stub says when the guest calls one.
///
/// A marker says the guest read a member; a stub there also says what the guest called it with,
/// where a marker used as a function pointer would end the run (D375).
extern "sysv64" fn report_member_call(names: u64, first: u64, second: u64, third: u64) -> u64 {
    use std::io::Write as _;

    let field = names / STUBBED_MEMBERS;
    let offset = (names % STUBBED_MEMBERS) * 8;
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "orbistoun: the guest called what handoff field {field} points at, offset {offset:#x}, with ({first:#x}, {second:#x}, {third:#x})"
    );
    let _ = err.flush();
    0
}

/// One table of stubs, mapped once, for every member of every stubbed field: one buffer rather
/// than a page per stub.
fn member_stubs() -> u64 {
    use std::sync::OnceLock;
    static STUBS: OnceLock<u64> = OnceLock::new();
    *STUBS.get_or_init(|| {
        let reporter: extern "sysv64" fn(u64, u64, u64, u64) -> u64 = report_member_call;
        let reporter = reporter as *const () as usize as u64;
        let count = STUBBED_FIELDS * STUBBED_MEMBERS;
        let mut code = Vec::with_capacity(count as usize * STUB_LEN);
        for names in 0..count {
            code.extend_from_slice(&naming_stub(names, reporter));
        }
        // Leaked: the guest holds these addresses for as long as it runs.
        let buffer = Box::leak(Box::new(
            crate::exec::ExecutableBuffer::new(&code).expect("the member stubs must be mappable"),
        ));
        buffer.address()
    })
}

/// Fills a mapped marker region so every word is a stub that names where it was read from.
///
/// Where [`fill_with_content_markers`] makes a read legible, this makes a call legible. A member
/// used as a data pointer still faults inside this table, which the fault reporter names.
///
/// # Safety
///
/// `base .. base + len` must be mapped writable and owned by the caller for as long as the
/// guest can reach it.
pub unsafe fn fill_with_member_stubs(base: u64, len: u64) {
    let stubs = member_stubs();
    let words = len / 8;
    for word in 0..words {
        let offset = word * 8;
        let field = offset / SENTINEL_STRIDE;
        let member = (offset % SENTINEL_STRIDE) / 8;
        let value = if field < STUBBED_FIELDS && member < STUBBED_MEMBERS {
            stubs + (field * STUBBED_MEMBERS + member) * STUB_LEN as u64
        } else {
            // Past what is stubbed, a marker still names itself.
            CONTENT_BASE + field * CONTENT_STRIDE + (offset % SENTINEL_STRIDE)
        };
        let Ok(at) = usize::try_from(base + offset) else {
            return;
        };
        // SAFETY: the caller guarantees the whole region is mapped writable, and `offset` is
        // inside it by construction.
        unsafe {
            std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), value);
        }
    }
}

/// Names for the globals a run pointed at a gadget stub, by index, published because a stub
/// arrives on a bare frame with no room for a context.
static GLOBAL_NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Publishes the names a gadget stub reports itself by.
pub fn install_global_names(names: Vec<String>) {
    let _ = GLOBAL_NAMES.set(names);
}

/// How many registers a gadget stub saves: `rax` and the six argument registers, plus `r10`.
pub const SAVED_GADGET_REGISTERS: usize = 8;

/// What a gadget stub says when the guest calls one.
///
/// A syscall gadget takes its number in `rax` and its fourth argument in `r10`, which no
/// argument-shaped report sees. This saves `rax` first and `r10` last and prints all eight,
/// which together say which convention the caller used (D377).
extern "sysv64" fn report_gadget_call(index: u64, saved: *const u64) -> u64 {
    use std::io::Write as _;

    let name = GLOBAL_NAMES
        .get()
        .and_then(|names| names.get(index as usize).cloned())
        .unwrap_or_else(|| format!("global #{index}"));
    // SAFETY: the stub that called this wrote exactly `SAVED_GADGET_REGISTERS` words into a buffer
    // this crate leaked and owns for the life of the process.
    let saved = unsafe { std::slice::from_raw_parts(saved, SAVED_GADGET_REGISTERS) };

    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "orbistoun: the guest called {name} with rax={:#x} rdi={:#x} rsi={:#x} rdx={:#x} rcx={:#x} r8={:#x} r9={:#x} r10={:#x}",
        saved[0], saved[1], saved[2], saved[3], saved[4], saved[5], saved[6], saved[7]
    );
    let _ = err.flush();
    0
}

/// The words a syscall gadget saves: the number and the six argument registers.
pub const SYSCALL_SAVED_WORDS: usize = 7;

/// The stack frame those words occupy, rounded up to keep `rsp` sixteen-byte aligned.
const SYSCALL_FRAME_BYTES: u8 = 64;

/// The bytes of a syscall gadget: the registers a syscall reads, and the ones it keeps.
///
/// The guest calls a pointer where a real system keeps `syscall; ret`, with the number in `rax`
/// and arguments in `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`. `syscall` preserves all but
/// `rax`, `rcx` and `r11`, so the argument registers are saved and restored around the Rust
/// dispatcher. The guest may arrive misaligned, and the dispatcher's `movaps` stores would fault,
/// so the gadget aligns `rsp`, keeping the old one in `rbp` (D384). Registers are saved into a
/// frame on the calling thread's own stack, so concurrent threads never share a save area.
///
/// ```text
/// push rdi rsi rdx r8 r9 r10     what a syscall would have preserved
/// push rbp                       the guest's, and what the alignment is remembered in
/// mov rbp, rsp
/// and rsp, -16                   whatever the guest was on, this is aligned
/// sub rsp, FRAME                 this thread's save area, still aligned
/// mov [rsp+0],  rax              the number, then the six the convention passes
/// mov [rsp+8],  rdi
/// mov [rsp+16], rsi
/// mov [rsp+24], rdx
/// mov [rsp+32], r10
/// mov [rsp+40], r8
/// mov [rsp+48], r9
/// mov rdi, rsp                   the save area, as the dispatcher's only argument
/// mov r11, imm64
/// call r11                       a call, not a jump: there is unwinding to do
/// mov rsp, rbp                   back to the guest's stack, however it was aligned
/// pop rbp
/// pop r10 r9 r8 rdx rsi rdi      in reverse
/// ret                            with the answer in rax, as the instruction leaves it
/// ```
fn syscall_gadget_code(dispatch: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(96);
    // push rdi / rsi / rdx / r8 / r9 / r10
    code.extend_from_slice(&[0x57, 0x56, 0x52]);
    code.extend_from_slice(&[0x41, 0x50, 0x41, 0x51, 0x41, 0x52]);
    // push rbp / mov rbp, rsp / and rsp, -16: aligned whatever the guest's alignment was. `rbp` is
    // callee-saved in both conventions, so the dispatcher hands it back.
    code.extend_from_slice(&[0x55]);
    code.extend_from_slice(&[0x48, 0x89, 0xE5]);
    code.extend_from_slice(&[0x48, 0x83, 0xE4, 0xF0]);
    // sub rsp, FRAME: a multiple of sixteen, so the alignment survives it
    code.extend_from_slice(&[0x48, 0x83, 0xEC, SYSCALL_FRAME_BYTES]);
    // mov [rsp+disp8], reg - ModRM mod=01 rm=100 (a SIB byte follows), SIB 0x24 (base rsp)
    for (rex, modrm, disp) in [
        (0x48_u8, 0x44_u8, 0_u8), // rax - the number
        (0x48, 0x7C, 8),          // rdi
        (0x48, 0x74, 16),         // rsi
        (0x48, 0x54, 24),         // rdx
        (0x4C, 0x54, 32),         // r10, where a syscall's fourth argument lives
        (0x4C, 0x44, 40),         // r8
        (0x4C, 0x4C, 48),         // r9
    ] {
        code.extend_from_slice(&[rex, 0x89, modrm, 0x24, disp]);
    }
    // mov rdi, rsp / mov r11, imm64 / call r11
    code.extend_from_slice(&[0x48, 0x89, 0xE7]);
    code.extend_from_slice(&[0x49, 0xBB]);
    code.extend_from_slice(&dispatch.to_le_bytes());
    code.extend_from_slice(&[0x41, 0xFF, 0xD3]);
    // mov rsp, rbp / pop rbp, then pop r10 / r9 / r8 / rdx / rsi / rdi
    code.extend_from_slice(&[0x48, 0x89, 0xEC]);
    code.extend_from_slice(&[0x5D]);
    code.extend_from_slice(&[0x41, 0x5A, 0x41, 0x59, 0x41, 0x58]);
    code.extend_from_slice(&[0x5A, 0x5E, 0x5F]);
    code.push(0xC3);
    code
}

/// The address of this run's syscall gadget, built once.
///
/// `dispatch` performs the call; it is passed in because its table belongs to a crate this one
/// does not depend on. Each call saves into its own thread's stack. `saved_registers` is how
/// many words the dispatcher reads; the frame holds [`SYSCALL_SAVED_WORDS`], and a dispatcher
/// wanting more is refused.
pub fn syscall_gadget(dispatch: u64, saved_registers: usize) -> Option<u64> {
    use std::sync::OnceLock;
    static GADGET: OnceLock<Option<u64>> = OnceLock::new();
    if saved_registers > SYSCALL_SAVED_WORDS {
        return None;
    }
    *GADGET.get_or_init(|| {
        let code = syscall_gadget_code(dispatch);
        let buffer = crate::exec::ExecutableBuffer::new(&code).ok()?;
        Some(Box::leak(Box::new(buffer)).address())
    })
}

/// One stub per global, each saving every register and naming itself.
///
/// ```text
/// mov r11, imm64      the save buffer for this stub
/// mov [r11+0],  rax   the syscall number, if that is what this is
/// mov [r11+8],  rdi   ..and the six argument registers
/// mov [r11+16], rsi
/// mov [r11+24], rdx
/// mov [r11+32], rcx
/// mov [r11+40], r8
/// mov [r11+48], r9
/// mov [r11+56], r10   the register a syscall uses where a call uses rcx
/// push rbp            the guest's alignment, whatever it is, remembered and made right
/// mov rbp, rsp
/// and rsp, -16
/// mov rdi, imm64      this stub's index
/// mov rsi, imm64      its save buffer
/// mov r11, imm64      the reporter
/// call r11
/// mov rsp, rbp        back to the guest's stack, and its answer is already in rax
/// pop rbp
/// ret
/// ```
///
/// The buffer is per stub, so two threads calling two gadgets keep separate reports. The stack
/// is aligned rather than tail-jumped, as for the syscall gadget (D384).
fn gadget_stub(index: u64, buffer: u64, reporter: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(96);
    code.extend_from_slice(&[0x49, 0xBB]);
    code.extend_from_slice(&buffer.to_le_bytes());
    // `mov [r11+disp8], reg`, one per saved register. The ModRM byte carries the register in
    // its middle field; REX.B selects r11 as the base and REX.R extends the register.
    for (rex, modrm, disp) in [
        (0x49_u8, 0x43_u8, 0_u8), // rax
        (0x49, 0x7B, 8),          // rdi
        (0x49, 0x73, 16),         // rsi
        (0x49, 0x53, 24),         // rdx
        (0x49, 0x4B, 32),         // rcx
        (0x4D, 0x43, 40),         // r8
        (0x4D, 0x4B, 48),         // r9
        (0x4D, 0x53, 56),         // r10
    ] {
        code.extend_from_slice(&[rex, 0x89, modrm, disp]);
    }
    // push rbp / mov rbp, rsp / and rsp, -16: the guest's alignment is not assumed, and `rbp` is
    // callee-saved in both conventions, so the reporter hands it back.
    code.extend_from_slice(&[0x55]);
    code.extend_from_slice(&[0x48, 0x89, 0xE5]);
    code.extend_from_slice(&[0x48, 0x83, 0xE4, 0xF0]);
    // mov rdi, imm64 - which global this is.
    code.extend_from_slice(&[0x48, 0xBF]);
    code.extend_from_slice(&index.to_le_bytes());
    // mov rsi, imm64 - where the registers were saved.
    code.extend_from_slice(&[0x48, 0xBE]);
    code.extend_from_slice(&buffer.to_le_bytes());
    // mov r11, imm64 / call r11
    code.extend_from_slice(&[0x49, 0xBB]);
    code.extend_from_slice(&reporter.to_le_bytes());
    code.extend_from_slice(&[0x41, 0xFF, 0xD3]);
    // mov rsp, rbp / pop rbp / ret: the reporter's answer is already in rax.
    code.extend_from_slice(&[0x48, 0x89, 0xEC]);
    code.extend_from_slice(&[0x5D]);
    code.push(0xC3);
    code
}

/// Bytes one gadget stub occupies, rounded so each starts on a sixteen-byte boundary.
const GADGET_STUB_LEN: usize = 96;

/// Addresses of `count` gadget stubs, mapped once and leaked, since the guest holds them for as
/// long as it runs.
pub fn gadget_stubs(count: usize) -> Vec<u64> {
    let saves: &'static mut [u64] = Box::leak(vec![0_u64; count * SAVED_GADGET_REGISTERS].into());
    let saves_at = saves.as_ptr() as u64;
    let reporter: extern "sysv64" fn(u64, *const u64) -> u64 = report_gadget_call;
    let reporter = reporter as *const () as usize as u64;

    let mut code = Vec::with_capacity(count * GADGET_STUB_LEN);
    for index in 0..count {
        let buffer = saves_at + (index * SAVED_GADGET_REGISTERS * 8) as u64;
        let mut one = gadget_stub(index as u64, buffer, reporter);
        one.resize(GADGET_STUB_LEN, 0xCC);
        code.extend_from_slice(&one);
    }
    let Ok(buffer) = crate::exec::ExecutableBuffer::new(&code) else {
        return Vec::new();
    };
    let base = Box::leak(Box::new(buffer)).address();
    (0..count)
        .map(|index| base + (index * GADGET_STUB_LEN) as u64)
        .collect()
}

/// Where the markers behind a field start.
///
/// A field marker says the guest used field `n`; once the guest reads through it, a zeroed page
/// names nothing. The page behind each field can hold its own markers, one per word, naming the
/// field and the offset read (D365). A separate base from [`SENTINEL_BASE`], adjacent so both
/// are recognisable.
pub const CONTENT_BASE: u64 = 0x0000_5E28_0000_0000;

/// How far apart consecutive fields' content markers sit.
///
/// Not [`SENTINEL_STRIDE`]: a guest may truncate a marker to 32 bits, and with one stride the
/// two depths would share a low half and become indistinguishable.
pub const CONTENT_STRIDE: u64 = 0x0010_0000;

/// Fills a mapped marker region so every word names where it was read from.
///
/// `base` is the region [`handoff_argument_block`] was given, and `len` its length. Each
/// eight-byte word becomes `CONTENT_BASE + field * SENTINEL_STRIDE + offset`, which
/// [`content_slot`] reads back.
///
/// # Safety
///
/// `base .. base + len` must be mapped writable and owned by the caller for as long as the
/// guest can reach it.
pub unsafe fn fill_with_content_markers(base: u64, len: u64) {
    let words = len / 8;
    for word in 0..words {
        let offset = word * 8;
        let field = offset / SENTINEL_STRIDE;
        let within = offset % SENTINEL_STRIDE;
        let value = CONTENT_BASE + field * CONTENT_STRIDE + within;
        let Ok(at) = usize::try_from(base + offset) else {
            return;
        };
        // SAFETY: the caller guarantees the whole region is mapped writable, and `offset` is inside it
        // by construction. Unaligned, since the caller's base need not be eight-byte aligned.
        unsafe {
            std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), value);
        }
    }
}

/// Reads a faulting address back as the field whose contents it came from, and the offset.
///
/// One level deeper than [`sentinel_slot`]: the guest read through a field and used what it
/// found.
#[must_use]
pub const fn content_slot(address: u64) -> Option<(usize, u64)> {
    let top = CONTENT_BASE + (ARGUMENT_BLOCK_SIZE as u64 / 8) * CONTENT_STRIDE;
    if address < CONTENT_BASE || address >= top {
        return None;
    }
    let offset = address - CONTENT_BASE;
    Some(((offset / CONTENT_STRIDE) as usize, offset % CONTENT_STRIDE))
}

/// Reads a faulting address back as the slot it came from, and what was added to it.
///
/// [`None`] outside the marker range, so an unrelated fault is never reported as naming a
/// field.
#[must_use]
pub const fn sentinel_slot(address: u64) -> Option<(usize, u64)> {
    let top = SENTINEL_BASE + (ARGUMENT_BLOCK_SIZE as u64 / 8) * SENTINEL_STRIDE;
    if address < SENTINEL_BASE || address >= top {
        return None;
    }
    let offset = address - SENTINEL_BASE;
    Some((
        (offset / SENTINEL_STRIDE) as usize,
        offset % SENTINEL_STRIDE,
    ))
}

/// Transfers control to guest code on a dedicated stack, with one argument.
///
/// The form a thread entry point `void *start(void *arg)` needs: the argument arrives in the
/// first System V argument register.
///
/// # Safety
///
/// As [`enter_guest`], and additionally: `argument` is passed to guest code unexamined,
/// so if the guest dereferences it, it must be a valid guest address.
pub unsafe fn enter_guest_with_argument(entry: u64, stack_pointer: u64, argument: u64) -> u64 {
    // SAFETY: the caller's contract, unchanged; a second argument of zero states what `rsi` holds
    // for a one-argument callee.
    unsafe { enter_guest_with_arguments(entry, stack_pointer, argument, 0) }
}

/// Transfers control to guest code on a dedicated stack, with two arguments.
///
/// The form `main(int argc, char **argv)` needs when a payload is entered at `main` (D343);
/// `rsi` is an input rather than a clobber.
///
/// # Safety
///
/// As [`enter_guest`], and additionally: both arguments are passed to guest code
/// unexamined, so if the guest dereferences either, it must be a valid guest address.
pub unsafe fn enter_guest_with_arguments(
    entry: u64,
    stack_pointer: u64,
    first: u64,
    second: u64,
) -> u64 {
    let returned: u64;
    // SAFETY: the block saves the host stack pointer in `r12` (callee-saved under System V, so the
    // guest restores it), switches to the guest stack, calls the guest, and switches back before
    // any compiler-generated code runs. Every register the guest may destroy is declared clobbered.
    // The caller guarantees `entry` and `stack_pointer`.
    unsafe {
        core::arch::asm!(
            "mov r12, rsp",
            "mov rsp, {stack}",
            "call {entry}",
            "mov rsp, r12",
            stack = in(reg) stack_pointer,
            entry = in(reg) entry,
            out("rax") returned,
            // System V caller-saved integer registers. `rsi` and `rdi` are callee-saved on Windows, which
            // is why they are listed.
            out("rcx") _,
            out("rdx") _,
            inout("rsi") second => _,
            inout("rdi") first => _,
            out("r8") _,
            out("r9") _,
            out("r10") _,
            out("r11") _,
            // Used to hold the host stack pointer across the call.
            out("r12") _,
            // Every vector register: `xmm6`-`xmm15` are callee-saved on Windows and caller-saved under
            // System V.
            out("xmm0") _,
            out("xmm1") _,
            out("xmm2") _,
            out("xmm3") _,
            out("xmm4") _,
            out("xmm5") _,
            out("xmm6") _,
            out("xmm7") _,
            out("xmm8") _,
            out("xmm9") _,
            out("xmm10") _,
            out("xmm11") _,
            out("xmm12") _,
            out("xmm13") _,
            out("xmm14") _,
            out("xmm15") _,
        );
    }
    returned
}

/// Transfers control to guest code on a dedicated stack, with three arguments.
///
/// The form an `InitOnce`-shaped callback needs, `(handle, parameter, context)` in `rdi`, `rsi`,
/// `rdx`; `rdx` is an input rather than a clobber.
///
/// # Safety
///
/// As [`enter_guest`], and additionally: all three arguments are passed to guest code unexamined,
/// so if the guest dereferences any of them, it must be a valid guest address.
#[cfg(target_arch = "x86_64")]
pub unsafe fn enter_guest_with_three_arguments(
    entry: u64,
    stack_pointer: u64,
    first: u64,
    second: u64,
    third: u64,
) -> u64 {
    let returned: u64;
    // SAFETY: as `enter_guest_with_arguments`, with `rdx` supplied as the third argument. The stack
    // switch and the clobber list are unchanged, and the caller guarantees `entry` and
    // `stack_pointer`.
    unsafe {
        core::arch::asm!(
            "mov r12, rsp",
            "mov rsp, {stack}",
            "call {entry}",
            "mov rsp, r12",
            stack = in(reg) stack_pointer,
            entry = in(reg) entry,
            out("rax") returned,
            out("rcx") _,
            inout("rdx") third => _,
            inout("rsi") second => _,
            inout("rdi") first => _,
            out("r8") _,
            out("r9") _,
            out("r10") _,
            out("r11") _,
            out("r12") _,
            out("xmm0") _,
            out("xmm1") _,
            out("xmm2") _,
            out("xmm3") _,
            out("xmm4") _,
            out("xmm5") _,
            out("xmm6") _,
            out("xmm7") _,
            out("xmm8") _,
            out("xmm9") _,
            out("xmm10") _,
            out("xmm11") _,
            out("xmm12") _,
            out("xmm13") _,
            out("xmm14") _,
            out("xmm15") _,
        );
    }
    returned
}

#[cfg(test)]
mod tests {
    /// A marker decodes back to the slot it came from, and a stray address decodes to nothing.
    #[test]
    fn a_marker_address_names_its_slot_and_a_stray_one_names_nothing() {
        use super::{ARGUMENT_BLOCK_SIZE, SENTINEL_BASE, SENTINEL_STRIDE, sentinel_slot};

        assert_eq!(sentinel_slot(SENTINEL_BASE), Some((0, 0)));
        assert_eq!(
            sentinel_slot(SENTINEL_BASE + 3 * SENTINEL_STRIDE),
            Some((3, 0))
        );
        // A displacement the guest added stays inside its own slot, and is reported.
        assert_eq!(
            sentinel_slot(SENTINEL_BASE + 3 * SENTINEL_STRIDE + 0x20),
            Some((3, 0x20)),
            "the slot it came from, and what was added to it"
        );

        assert_eq!(sentinel_slot(0), None, "a null fault is not a field");
        assert_eq!(
            sentinel_slot(SENTINEL_BASE - 1),
            None,
            "just below the range is outside it"
        );
        let top = SENTINEL_BASE + (ARGUMENT_BLOCK_SIZE as u64 / 8) * SENTINEL_STRIDE;
        assert_eq!(
            sentinel_slot(top),
            None,
            "one past the last slot is outside it - the block is not unbounded"
        );
    }

    /// Every slot holds a different marker, or the technique answers nothing.
    #[test]
    fn every_sentinel_slot_is_distinct_and_decodes_to_itself() {
        use super::{ARGUMENT_BLOCK_SIZE, sentinel_argument_block, sentinel_slot};

        let base = sentinel_argument_block();
        for slot in [0_usize, 1, 7, ARGUMENT_BLOCK_SIZE / 8 - 1] {
            // SAFETY: the block is a leaked array of `ARGUMENT_BLOCK_SIZE / 8` `u64`, and
            // every index used here is inside it.
            let cell = unsafe { std::ptr::with_exposed_provenance::<u64>(base as usize).add(slot) };
            // SAFETY: `cell` points at an initialised `u64` inside that same leaked array.
            let held = unsafe { std::ptr::read(cell) };
            assert_eq!(
                sentinel_slot(held),
                Some((slot, 0)),
                "slot {slot} must hold the marker that names it"
            );
        }
    }

    use super::enter_guest;
    use crate::{emit_return_constant, exec::ExecutableBuffer};
    use orbistoun_mem::stack::GuestStack;

    /// Far from anything a normal process maps.
    const TEST_STACK_BASE: u64 = 0x0000_6500_0000_0000;

    /// Control transfers to generated code on a guest stack and comes back with its value.
    #[test]
    fn control_transfers_to_generated_code_on_a_guest_stack_and_comes_back() {
        // Real machine code, a real stack switch, and a value carried back in `rax`: only execution
        // proves the register discipline.
        let code = emit_return_constant(0x0BAD_C0DE);
        let buffer = ExecutableBuffer::new(&code).expect("map executable memory");
        let stack = GuestStack::reserve(TEST_STACK_BASE, 64 * 1024).expect("reserve a stack");

        // SAFETY: `buffer` holds mapped executable code that returns immediately, and `stack` is a
        // mapped, writable, sixteen-byte-aligned guest stack with a guard page beneath it. Both outlive
        // the call.
        let got = unsafe { enter_guest(buffer.address(), stack.initial_pointer()) };
        assert_eq!(got, 0x0BAD_C0DE, "the guest return value must survive");
    }

    /// An argument reaches the guest in the first register.
    #[test]
    fn an_argument_reaches_the_guest_in_the_first_register() {
        // A thread body gets its context in `rdi`. `48 89 F8` is `mov rax, rdi`; `C3` is `ret`.
        let code = [0x48, 0x89, 0xF8, 0xC3];
        let buffer = ExecutableBuffer::new(&code).expect("map executable memory");
        let stack =
            GuestStack::reserve(TEST_STACK_BASE + 0x200_0000, 64 * 1024).expect("reserve a stack");

        // SAFETY: mapped executable code that reads one register and returns, on a mapped, aligned
        // guest stack. The argument is never dereferenced.
        let got = unsafe {
            super::enter_guest_with_argument(buffer.address(), stack.initial_pointer(), 0xFEED)
        };
        assert_eq!(got, 0xFEED, "the argument must arrive in rdi");
    }

    /// Three arguments reach guest code and their sum comes back, as for a `call_once` initialiser.
    ///
    /// `48 89 F8` mov rax, rdi; `48 01 F0` add rax, rsi; `48 01 D0` add rax, rdx; `C3` ret.
    #[test]
    fn three_arguments_reach_the_guest_and_their_result_returns() {
        let code = [0x48, 0x89, 0xF8, 0x48, 0x01, 0xF0, 0x48, 0x01, 0xD0, 0xC3];
        let buffer = ExecutableBuffer::new(&code).expect("map executable memory");
        let stack =
            GuestStack::reserve(TEST_STACK_BASE + 0x400_0000, 64 * 1024).expect("reserve a stack");
        // SAFETY: mapped executable code that reads three registers and returns their sum, on a mapped,
        // aligned guest stack; it dereferences none of the arguments.
        let got = unsafe {
            super::enter_guest_with_three_arguments(
                buffer.address(),
                stack.initial_pointer(),
                3,
                4,
                5,
            )
        };
        assert_eq!(got, 12, "rdi + rsi + rdx must come back in rax");
    }

    /// A reporting stub shifts the arguments and tail-calls, asserted byte by byte.
    #[test]
    fn a_reporting_stub_shifts_the_arguments_and_tail_calls() {
        let code = super::reporting_stub(9);
        assert_eq!(&code[..3], &[0x48, 0x89, 0xD1], "mov rcx, rdx");
        assert_eq!(&code[3..6], &[0x48, 0x89, 0xF2], "mov rdx, rsi");
        assert_eq!(&code[6..9], &[0x48, 0x89, 0xFE], "mov rsi, rdi");
        assert_eq!(&code[9..11], &[0x48, 0xBF], "mov rdi, imm64");
        assert_eq!(
            &code[11..19],
            &9_u64.to_le_bytes(),
            "the slot it stands for"
        );
        assert_eq!(&code[19..21], &[0x49, 0xBB], "mov r11, imm64");
        assert_eq!(&code[29..], &[0x41, 0xFF, 0xE3], "jmp r11");
    }

    /// Two reporting stubs differ only in the slot they name.
    #[test]
    fn two_reporting_stubs_differ_only_in_the_slot_they_name() {
        let (first, second) = (super::reporting_stub(0), super::reporting_stub(1));
        assert_eq!(first[..11], second[..11], "the same shift");
        assert_ne!(first[11..19], second[11..19], "a different slot");
        assert_eq!(first[19..], second[19..], "the same reporter");
    }

    /// The handoff block holds the resolver in field zero and markers after it.
    #[test]
    fn the_handoff_block_holds_the_resolver_first_and_markers_after() {
        let at = super::handoff_argument_block(
            0xABCD,
            super::UnknownFields::Markers {
                base: super::SENTINEL_BASE,
            },
            &[],
        );
        // SAFETY: the block is a leaked page this crate owns for the life of the process.
        let slots = unsafe {
            std::slice::from_raw_parts(
                std::ptr::with_exposed_provenance::<u64>(at as usize),
                super::ARGUMENT_BLOCK_SIZE / 8,
            )
        };
        assert_eq!(
            slots[0], 0xABCD,
            "the resolver, which is the part that is known"
        );
        assert_eq!(
            super::sentinel_slot(slots[1]),
            Some((1, 0)),
            "and every field after it still names itself"
        );
        assert_eq!(super::sentinel_slot(slots[5]), Some((5, 0)));
        assert_eq!(slots[6], 0, "word 6 is zero in the measured D208 layout");
        assert_eq!(slots[7], 0, "word 7 is zero in the measured D208 layout");
    }

    /// The gadget reads `r10` for the fourth argument and preserves what a syscall preserves
    /// (D377).
    #[test]
    fn a_syscall_gadget_reads_r10_and_preserves_what_a_syscall_preserves() {
        let code = super::syscall_gadget_code(0x2000);
        assert_eq!(
            &code[0..9],
            &[0x57, 0x56, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52],
            "push rdi rsi rdx r8 r9 r10 - what a syscall would have preserved"
        );
        // The alignment (D384): the guest may arrive misaligned, and the dispatcher's `movaps` would
        // fault on its own frame.
        assert_eq!(code[9], 0x55, "push rbp - the guest's, and the old rsp");
        assert_eq!(&code[10..13], &[0x48, 0x89, 0xE5], "mov rbp, rsp");
        assert_eq!(
            &code[13..17],
            &[0x48, 0x83, 0xE4, 0xF0],
            "and rsp, -16 - aligned whatever it was"
        );
        assert_eq!(
            &code[17..21],
            &[0x48, 0x83, 0xEC, 0x40],
            "sub rsp, 64 - this thread's save area, a multiple of sixteen"
        );
        let stores = &code[21..56];
        assert_eq!(
            &stores[0..5],
            &[0x48, 0x89, 0x44, 0x24, 0],
            "mov [rsp], rax"
        );
        assert_eq!(
            &stores[20..25],
            &[0x4C, 0x89, 0x54, 0x24, 32],
            "mov [rsp+32], r10 - the fourth argument, not rcx"
        );
        assert_eq!(
            &code[56..59],
            &[0x48, 0x89, 0xE7],
            "mov rdi, rsp - the save area"
        );
        assert_eq!(&code[59..61], &[0x49, 0xBB], "mov r11, imm64");
        assert_eq!(&code[61..69], &0x2000_u64.to_le_bytes(), "the dispatcher");
        assert_eq!(&code[69..72], &[0x41, 0xFF, 0xD3], "call r11, not a jump");
        assert_eq!(&code[72..75], &[0x48, 0x89, 0xEC], "mov rsp, rbp");
        assert_eq!(code[75], 0x5D, "pop rbp - the guest's own value back");
        assert_eq!(
            &code[76..85],
            &[0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x5E, 0x5F],
            "pop r10 r9 r8 rdx rsi rdi - in reverse"
        );
        assert_eq!(
            code[85], 0xC3,
            "ret, with the answer where the instruction leaves it"
        );
    }

    /// No two threads share a save area: the gadget names only the dispatcher's address and stores
    /// through the calling thread's `rsp`.
    #[test]
    fn the_gadget_saves_through_the_callers_stack_and_names_no_buffer() {
        let code = super::syscall_gadget_code(0x2000);
        let absolutes = code.windows(2).filter(|w| *w == [0x49, 0xBB]).count()
            + code.windows(2).filter(|w| *w == [0x48, 0xBF]).count();
        assert_eq!(
            absolutes, 1,
            "one imm64 - the dispatcher - and no save buffer"
        );
        for store in code[21..56].chunks(5) {
            assert_eq!(
                &store[1..4],
                &[0x89, store[2], 0x24],
                "every save is [rsp+disp]"
            );
        }
        assert_eq!(
            super::syscall_gadget(0x2000, super::SYSCALL_SAVED_WORDS + 1),
            None,
            "a dispatcher wanting more than the frame holds is refused"
        );
    }

    /// The dispatcher is entered on an aligned stack however the guest arrived (D384).
    #[test]
    fn the_gadget_aligns_whatever_stack_it_was_entered_on() {
        let code = super::syscall_gadget_code(0x2000);
        let mask = code
            .windows(4)
            .position(|w| w == [0x48, 0x83, 0xE4, 0xF0])
            .expect("the gadget masks rsp");
        let call = code
            .windows(3)
            .position(|w| w == [0x41, 0xFF, 0xD3])
            .expect("the gadget calls the dispatcher");
        assert!(
            mask < call,
            "the mask has to come before the call, or it aligns nothing"
        );
        let restore = code
            .windows(3)
            .position(|w| w == [0x48, 0x89, 0xEC])
            .expect("the gadget puts the guest's stack back");
        assert!(
            call < restore,
            "and the guest's own stack comes back after it, or the pops read rubble"
        );
    }

    /// The pushes and the pops match.
    #[test]
    fn the_gadget_restores_exactly_what_it_saved() {
        let code = super::syscall_gadget_code(0x2000);
        let pushes = &code[0..9];
        let pops = &code[76..85];
        // Six registers each way: three one-byte forms and three two-byte extended ones.
        assert_eq!(pushes.len(), pops.len());
        // The extended registers are pushed low-to-high and popped high-to-low.
        assert_eq!(
            &pushes[3..9],
            &[0x41, 0x50, 0x41, 0x51, 0x41, 0x52],
            "r8 r9 r10"
        );
        assert_eq!(
            &pops[0..6],
            &[0x41, 0x5A, 0x41, 0x59, 0x41, 0x58],
            "r10 r9 r8"
        );
    }

    /// A gadget stub saves every register a syscall uses (D377).
    #[test]
    fn a_gadget_stub_saves_every_register_a_syscall_uses() {
        let code = super::gadget_stub(3, 0x1000, 0x2000);
        assert_eq!(&code[..2], &[0x49, 0xBB], "mov r11, imm64");
        assert_eq!(&code[2..10], &0x1000_u64.to_le_bytes(), "the save buffer");

        // Eight four-byte stores, in the order the reporter reads them back.
        let stores = &code[10..42];
        assert_eq!(&stores[0..4], &[0x49, 0x89, 0x43, 0], "mov [r11+0], rax");
        assert_eq!(&stores[4..8], &[0x49, 0x89, 0x7B, 8], "mov [r11+8], rdi");
        assert_eq!(
            &stores[28..32],
            &[0x4D, 0x89, 0x53, 56],
            "mov [r11+56], r10"
        );

        // Aligned first, as for the syscall gadget.
        assert_eq!(code[42], 0x55, "push rbp");
        assert_eq!(&code[43..46], &[0x48, 0x89, 0xE5], "mov rbp, rsp");
        assert_eq!(&code[46..50], &[0x48, 0x83, 0xE4, 0xF0], "and rsp, -16");

        assert_eq!(&code[50..52], &[0x48, 0xBF], "mov rdi, imm64");
        assert_eq!(&code[52..60], &3_u64.to_le_bytes(), "which global this is");
        assert_eq!(&code[60..62], &[0x48, 0xBE], "mov rsi, imm64");
        assert_eq!(
            &code[62..70],
            &0x1000_u64.to_le_bytes(),
            "and where to read them"
        );
        assert_eq!(&code[70..72], &[0x49, 0xBB], "mov r11, imm64");
        assert_eq!(&code[72..80], &0x2000_u64.to_le_bytes(), "the reporter");
        assert_eq!(&code[80..83], &[0x41, 0xFF, 0xD3], "call r11");
        assert_eq!(&code[83..86], &[0x48, 0x89, 0xEC], "mov rsp, rbp");
        assert_eq!(code[86], 0x5D, "pop rbp");
        assert_eq!(
            code[87], 0xC3,
            "ret - the reporter's answer is already in rax, which is what the guest reads"
        );
    }

    /// Each stub reads its own buffer, so two threads cannot overwrite each other's report.
    #[test]
    fn two_gadget_stubs_save_to_different_places() {
        let first = super::gadget_stub(0, 0x1000, 0x9000);
        let second = super::gadget_stub(1, 0x2000, 0x9000);
        assert_ne!(first[2..10], second[2..10], "different buffers");
        assert_eq!(first[70..], second[70..], "the same reporter");
    }

    /// A named field replaces what the block would have held (D375).
    #[test]
    fn a_named_field_replaces_what_the_block_would_have_held() {
        // A separate block from the real one, which is built once per process; this asserts the
        // composition rule with the values the worker would pass.
        let composed = |named: &[[u64; 2]]| {
            let mut block = [0_u64; 8];
            for (slot, cell) in block.iter_mut().enumerate() {
                *cell = if slot == 0 {
                    0xABCD
                } else {
                    super::SENTINEL_BASE + (slot as u64) * super::SENTINEL_STRIDE
                };
            }
            for [field, value] in named {
                if let Some(cell) = block.get_mut(*field as usize) {
                    *cell = *value;
                }
            }
            block
        };

        let untouched = composed(&[]);
        assert_eq!(super::sentinel_slot(untouched[2]), Some((2, 0)));

        let swept = composed(&[[2, 0]]);
        assert_eq!(swept[2], 0, "the named value, not the marker");
        assert_eq!(swept[0], 0xABCD, "and nothing else moved");
        assert_eq!(super::sentinel_slot(swept[3]), Some((3, 0)));

        let replaced = composed(&[[0, 0x1234]]);
        assert_eq!(
            replaced[0], 0x1234,
            "naming field zero replaces the resolver, which is a thing somebody may want to try"
        );
    }

    /// A field marker and a content marker differ in their low half, so a value truncated to 32
    /// bits still says which depth it came from.
    #[test]
    fn a_field_marker_and_a_content_marker_differ_in_their_low_half() {
        let field = super::SENTINEL_BASE + 2 * super::SENTINEL_STRIDE;
        let content = super::CONTENT_BASE + 2 * super::CONTENT_STRIDE;
        assert_ne!(field as u32, content as u32);
    }

    /// Each depth decodes to itself and refuses the other's addresses.
    #[test]
    fn each_depth_decodes_its_own_addresses_and_not_the_others() {
        let field = super::SENTINEL_BASE + 3 * super::SENTINEL_STRIDE;
        let content = super::CONTENT_BASE + 3 * super::CONTENT_STRIDE + 0x18;

        assert_eq!(super::sentinel_slot(field), Some((3, 0)));
        assert_eq!(super::content_slot(field), None, "a field is not a content");

        assert_eq!(super::content_slot(content), Some((3, 0x18)));
        assert_eq!(
            super::sentinel_slot(content),
            None,
            "and a content is not a field"
        );
    }

    /// An address belonging to neither depth names nothing.
    #[test]
    fn an_address_belonging_to_neither_depth_names_nothing() {
        assert_eq!(super::content_slot(0x4000_0000_0000), None);
        assert_eq!(super::sentinel_slot(0x4000_0000_0000), None);
    }

    /// Filling makes every word name the field and offset it sits at.
    #[test]
    fn filling_makes_every_word_name_where_it_came_from() {
        // Two fields' worth, enough to show the stride and the offset apart.
        let mut region = vec![0_u64; 2 * (super::SENTINEL_STRIDE as usize / 8)];
        let base = region.as_mut_ptr() as usize as u64;
        // SAFETY: `region` is a live, writable allocation of exactly this length, and it
        // outlives the call.
        unsafe { super::fill_with_content_markers(base, 2 * super::SENTINEL_STRIDE) };

        assert_eq!(super::content_slot(region[0]), Some((0, 0)));
        assert_eq!(super::content_slot(region[1]), Some((0, 8)));
        let second_field = super::SENTINEL_STRIDE as usize / 8;
        assert_eq!(super::content_slot(region[second_field]), Some((1, 0)));
        assert_eq!(
            super::content_slot(region[second_field + 3]),
            Some((1, 0x18))
        );
    }

    /// The host stack is intact after a guest call.
    #[test]
    fn the_host_stack_is_intact_afterwards() {
        // If `r12` were not restored or the guest stack leaked into host frames, host locals would be
        // corrupted.
        let before = vec![1_u64, 2, 3];
        let code = emit_return_constant(7);
        let buffer = ExecutableBuffer::new(&code).expect("map executable memory");
        let stack =
            GuestStack::reserve(TEST_STACK_BASE + 0x100_0000, 64 * 1024).expect("reserve a stack");

        // SAFETY: as above - mapped executable code and a mapped, aligned guest stack.
        let got = unsafe { enter_guest(buffer.address(), stack.initial_pointer()) };

        assert_eq!(got, 7);
        assert_eq!(before, vec![1, 2, 3], "host locals must be untouched");
    }
}
