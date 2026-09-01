//! Entering the guest: the host-to-guest direction of the same boundary.
//!
//! The rest of this crate proves a guest can call *us*. This is the other way round,
//! and it is the last mechanical step before guest code executes.
//!
//! # Why the stack has to be switched, not borrowed
//!
//! Running guest code on the host thread's stack works right up until the guest
//! overruns it, at which point it corrupts the host frames sitting below - and the
//! crash surfaces inside the emulator rather than in the guest. Switching means an
//! overrun hits a guard page instead, which says what actually happened.
//!
//! # Register discipline, which is where this gets subtle on Windows
//!
//! The guest follows System V, so it may destroy `rax`, `rcx`, `rdx`, `rsi`, `rdi`,
//! `r8`-`r11` and every `xmm` register. Windows host code expects `rsi`, `rdi` and
//! `xmm6`-`xmm15` to survive a call. Those sets disagree, so **every** System V
//! caller-saved register is declared clobbered here - letting the compiler save what it
//! needs. Omitting them would corrupt host state on Windows only, silently, and long
//! after this returns.
//!
//! `r12` holds the host stack pointer across the call because it is callee-saved under
//! System V: the guest is obliged to give it back.
//!
//! # Where the process image lives, and why not here
//!
//! A real process entry point expects a **process stack image** - argument count,
//! argument and environment pointers, and an auxiliary vector - not a return address.
//! That image is built, and it is built in `orbistoun-loader::process`, not here: this
//! crate transfers control and knows nothing about what the stack should contain.
//!
//! The split is deliberate. This was a spike that answered whether the call boundary
//! works at all, and it kept that scope; deferring the image was later shown to be wrong
//! on its own terms - giving the entry point a *defined* first argument made two
//! unrelated titles fault at the identical offset, having been reading through a stray
//! host pointer and getting plausible garbage (D152).

/// Transfers control to guest code on a dedicated stack.
///
/// Returns whatever the guest leaves in `rax`, should it return at all. Most guests
/// will not: they fault, or they call an unimplemented import and take a path that
/// never comes back. That is expected, and it is why this runs in a worker process.
///
/// # Safety
///
/// This executes arbitrary guest machine code, so nothing about it can be checked by
/// the compiler. The caller must ensure:
///
/// - `entry` points at mapped, executable, fully relocated guest code.
/// - `stack_pointer` is the top of a mapped, writable, guest-owned stack, aligned to
///   sixteen bytes, with room beneath it for the guest to grow into.
/// - No host state that must survive is reachable only through registers the guest may
///   destroy.
pub unsafe fn enter_guest(entry: u64, stack_pointer: u64) -> u64 {
    // SAFETY: the caller's guarantees are exactly the ones the general form needs, and
    // the argument is a block this crate owns for as long as the process runs.
    unsafe { enter_guest_with_argument(entry, stack_pointer, process_argument_block()) }
}

/// Transfers control to a **process** entry point, and never comes back.
///
/// # Why this cannot be a call
///
/// The System V ABI puts `rsp` sixteen-byte aligned at the entry point and has it point
/// at the argument count. A `call` pushes a return address, which lands at exactly the
/// address the count must occupy and leaves the stack eight past alignment. The two
/// conventions are not reconcilable by adjusting an offset - a process is *jumped* to.
///
/// So there is nothing to return to. A program leaves by calling exit. An entry point
/// that executes `ret` pops the argument count and jumps to it, faulting on a very small
/// address - which the fault reporter names, and which is a true description of what
/// happened rather than a mystery.
///
/// Losing the return value costs nothing here: the fault handler and the time limit each
/// persist the call trace from inside the guest's own thread, because those are the paths
/// that actually fire (D066).
///
/// `rbp` is zeroed, which is the standard's way of marking the end of the frame chain -
/// a debugger walking back from guest code stops there instead of following whatever the
/// host left in that register.
///
/// # Safety
///
/// Executes arbitrary guest machine code and never returns. `entry` must point at mapped,
/// executable, fully relocated code; `stack_pointer` must be the sixteen-byte-aligned
/// address of a written process image inside a mapped, writable guest stack. `argument`
/// is passed unexamined, so if the guest dereferences it, it must be valid.
#[cfg(target_arch = "x86_64")]
pub unsafe fn enter_process(entry: u64, stack_pointer: u64, argument: u64) -> ! {
    // SAFETY: the caller guarantees the entry, the stack and the argument. Nothing after
    // this executes on this thread, so no host state needs to survive and no clobber list
    // is required - which is why `noreturn` is correct rather than convenient.
    //
    // The entry and the argument are pinned to explicit registers so the compiler cannot
    // place one of them in `rdi` and have the argument move destroy it before the jump.
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
/// **Not a capability check - an architecture one.** Guest code is x86-64 and runs
/// natively, which is the whole architecture (principle 12 rules out an execution-backend
/// abstraction on purpose). A build for any other architecture can parse containers, name
/// imports, read traces and answer every analysis question, and can never run a guest.
///
/// This exists so that limit can be *reported* rather than hit: `enter_process` on a
/// non-x86-64 target is `unimplemented!()`, and a panic is not an honest failure, it is a
/// crash (principle 3). One `aarch64-apple-darwin` artifact is published, so this is a
/// real user reaching a real dead end, not a hypothetical (D208).
#[must_use]
pub const fn can_execute_guests() -> bool {
    cfg!(target_arch = "x86_64")
}

/// How much zeroed memory the process argument block holds.
///
/// A page. Generous, because the point is that any offset the entry point reads at is
/// inside it, and nothing here knows which offsets those are.
pub const ARGUMENT_BLOCK_SIZE: usize = 4096;

/// A zeroed block for the entry point to read its process arguments out of.
///
/// # Why this exists, and how it was found
///
/// A process entry point is not an ordinary function. It expects a **process argument
/// block** - argument count, argument and environment pointers, an auxiliary vector -
/// and it reads through the pointer it is given in the first argument register.
///
/// That was known and deliberately not built. What was not known is that the entry point
/// **dereferences that register immediately**, and it was only found by accident: adding
/// an argument to this call changed `rdi` from an undefined clobber to an explicit zero,
/// and two unrelated titles went from reaching thousands of bytes into their own code to
/// faulting with `read of 0x0` at the identical offset, `image+0x7a`.
///
/// So the previous behaviour was not "no argument". It was **whatever the compiler left
/// in `rdi`** - a stray host pointer the guest dereferenced and got plausible-looking
/// garbage from. That is precisely the failure mode principle 3 exists to prevent, and it
/// had been passing as progress (D152).
///
/// Zeroed and never written, for the same reason a thread handle's block is: the real
/// layout is not known from any lawful source, so every field reads as zero rather than
/// as something invented. A guest reading a count gets none; a guest reading a pointer
/// gets null and can check it.
pub fn process_argument_block() -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        let block: Box<[u64; ARGUMENT_BLOCK_SIZE / 8]> = Box::new([0; ARGUMENT_BLOCK_SIZE / 8]);
        std::ptr::from_mut(Box::leak(block)) as usize as u64
    })
}

/// Where a sentinel block's markers start.
///
/// Chosen to be canonical, certainly unmapped, and **recognisable on sight** in a fault
/// report - an address that could plausibly be a real pointer would leave a reader unsure
/// whether they were looking at a marker or at something the guest computed.
pub const SENTINEL_BASE: u64 = 0x0000_5E27_0000_0000;

/// How far apart consecutive markers sit.
///
/// Wide on purpose. The guest may add a small displacement to whatever it takes out of the
/// block before using it - `call [rax+8]`, a field within a sub-structure - and a wide
/// stride means the arithmetic still lands inside the slot it came from, so the fault names
/// both **which** marker was used and **what was added to it**.
pub const SENTINEL_STRIDE: u64 = 0x1000;

/// A block whose every slot holds a distinct, identifiable marker.
///
/// # What this is for
///
/// [`process_argument_block`] is zeroed, which is the honest answer when nothing is known
/// about a layout - but it answers nothing either. A guest that reads a pointer out of it
/// gets null, and null tells you the guest wanted *a* pointer, not **which field** it
/// wanted.
///
/// This fills every slot with a different unmapped address instead. The guest reads one,
/// uses it, and faults - and the faulting address says exactly which slot it came from.
/// One boot identifies a field precisely, rather than one boot per candidate offset, which
/// is the same planted-value trick the argument sweeps already use (D283, D286, D308).
///
/// It is a **diagnostic**: nothing about it is a claim as to what the layout is, and a run
/// under it is not an ordinary run. `orbistoun-env` records that, so a verdict taken here
/// is never compared against one that was not (D181, D224).
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
/// # The question this asks that markers cannot
///
/// [`sentinel_argument_block`] answers *which* field the entry point uses, by faulting on
/// it. That is one field per boot, and it stops the guest dead every time.
///
/// This answers a different question: **if every field it asks for answers harmlessly, how
/// far does it get?** Every slot holds the address of the same `xor eax, eax; ret`, so any
/// field the guest calls returns zero and control comes back. A guest that then goes on to
/// call its imports has told us the structure is a table of functions and nothing more; one
/// that faults somewhere new has told us which field needed to be data rather than code
/// (D308).
///
/// **Neither is a claim about the layout.** Both are diagnostics, and a run under either is
/// not an ordinary run.
pub fn answering_argument_block() -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        // **A whole page of `ret`, not a three-byte stub.** The first attempt was exactly
        // three bytes in a page of zeros, and the guest entered it at `+0xa` - so it ran off
        // the end into `00 00`, which decodes as `add [rax], al` and faulted on a write. The
        // diagnostic reported the guest doing something wrong when the wrong thing was the
        // diagnostic. Filling the page means any entry point into it returns, and only then
        // does a fault afterwards say something about the guest (D308).
        let mut code = vec![0xC3_u8; ARGUMENT_BLOCK_SIZE];
        // And at the front, answer zero rather than whatever was left in `rax`.
        code[..3].copy_from_slice(&[0x31, 0xC0, 0xC3]);
        // Leaked deliberately: the guest holds these addresses for as long as it runs, and a
        // buffer freed underneath it would turn a diagnostic into a use-after-free.
        let stub = Box::leak(Box::new(
            crate::exec::ExecutableBuffer::new(&code).expect("a page of stub must be mappable"),
        ));
        // **Two kinds of field, so two kinds of answer.** Handing every slot the same
        // executable address could not tell a field that is *called* from one that is
        // *written through*: the guest called slot zero, returned, and then wrote through
        // something - into read-execute memory, which faulted and said nothing about which
        // slot it was.
        //
        // So slot zero, which every payload measured calls first, gets the returning page;
        // every other slot gets its own writable page. A write now succeeds and the guest
        // carries on, while a *call* to one of them faults on an instruction fetch at an
        // address that names the slot (D308).
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
/// Printed rather than accumulated, and flushed per line: the guest's very next act is
/// usually the fault this was installed to explain, so a line held in a buffer is a line
/// nobody reads.
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
/// # The question the other two blocks cannot ask
///
/// [`sentinel_argument_block`] answers *which* field the guest uses, by faulting on it -
/// one field per run, and the run ends there. [`answering_argument_block`] answers *how far
/// it gets* when every field answers harmlessly - but it answers zero to everything and so
/// says nothing about what was asked.
///
/// This one answers harmlessly **and** says what was asked: the slot, and the three
/// arguments the guest passed. That is the difference between knowing the entry point calls
/// field zero and knowing it calls field zero with a pointer - which, if the pointer turns
/// out to name a string, says the field is a resolver and says what it is resolving (D365).
///
/// # How a stub knows which slot it is
///
/// One emitted stub per slot, each shifting the guest's arguments along by one register and
/// putting its own index in the first. Reading the slot out of a register the reporter could
/// not name would have needed assembly on the other side; shifting needs nine bytes.
///
/// ```text
/// mov rcx, rdx     the guest's third argument moves to the fourth
/// mov rdx, rsi     its second to the third
/// mov rsi, rdi     its first to the second
/// mov rdi, imm64   the slot index takes the first
/// mov r11, imm64   the reporter's address
/// jmp r11
/// ```
///
/// The guest's fourth argument and beyond are dropped, which is stated rather than hidden:
/// this reports the shape of a call, not its every operand.
///
/// A diagnostic, exactly as the other two are. Nothing here is a claim about the layout, and
/// a run under it is not an ordinary run.
pub fn reporting_argument_block() -> u64 {
    use std::sync::OnceLock;
    static BLOCK: OnceLock<u64> = OnceLock::new();
    *BLOCK.get_or_init(|| {
        let mut block: Box<[u64; ARGUMENT_BLOCK_SIZE / 8]> = Box::new([0; ARGUMENT_BLOCK_SIZE / 8]);
        for (slot, cell) in block.iter_mut().enumerate() {
            // Leaked deliberately, like every other stub here: the guest holds the address
            // for as long as it runs, and freeing one underneath it would turn a diagnostic
            // into a use-after-free.
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
/// # What is known, and what is still a marker
///
/// Field zero is a function pointer, and the guest calls it with a module number, a string,
/// and somewhere to put an answer. Measured, not assumed: the string it passed was
/// `sceKernelDlsym`, read out of the payload's own read-only data (D365). So field zero is
/// given the resolver, and everything after it keeps the markers - because nothing has
/// established what any of it holds, and a marker is what makes the *next* field the guest
/// wants name itself.
///
/// This is the one block here that is not purely a diagnostic. Half of it is knowledge and
/// half of it is a question, which is exactly the state of the thing it describes.
///
/// # Why the unknown fields want to be *mapped* markers
///
/// [`sentinel_argument_block`]'s markers are unmapped, so any use of one stops the run.
/// That is right when the question is "which field", and wrong here: a runtime that reads
/// six fields would need six runs to get past them, and each run says only what the last
/// one already implied.
///
/// So `unknown_base` may name a region the caller has **mapped and zeroed**. A field read
/// as a pointer then yields zero - which a correct program checks - and the runtime carries
/// on to whatever it needs next, while the address it read from still says which field it
/// was. Passing [`SENTINEL_BASE`] with nothing mapped there is the older, stricter
/// behaviour and stays available.
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
        // **Last, so a named field wins.** A sweep names one field and leaves the rest to the
        // markers; naming field zero deliberately replaces the resolver, which is a thing
        // somebody may want to try and should not have to edit code to try (D375).
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
/// **Two answers, and neither is a claim about the layout.** A marker says which field the
/// guest used, by faulting on it or by turning up in an argument; a zero says nothing about
/// which, and is the value a correct program can *check* - so a runtime that reads a field
/// it does not strictly need carries on rather than corrupting itself with a marker it
/// mistook for a handle (D368).
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

/// The bytes of one reporting stub. Split out so the encoding is testable without mapping.
fn reporting_stub(slot: u64) -> Vec<u8> {
    let reporter: extern "sysv64" fn(u64, u64, u64, u64) -> u64 = report_call;
    naming_stub(slot, reporter as *const () as usize as u64)
}

/// Bytes of one stub, which is exactly what the six instructions below need.
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
/// `r11` is caller-saved and not an argument register, so loading it clobbers nothing the
/// reporter is about to read. The jump is a tail call, so the reporter returns straight to
/// the guest.
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

/// Fields whose contents get stubs rather than markers.
///
/// Sixteen. A runtime that reads past the sixteenth field of a structure it was handed is
/// doing something this has no reason to anticipate, and every field past this still names
/// itself as a marker.
pub const STUBBED_FIELDS: u64 = 16;

/// Members per field that get one.
///
/// Sixty-four eight-byte members - half a kilobyte into each structure, which is past the
/// end of anything a handoff structure plausibly is.
pub const STUBBED_MEMBERS: u64 = 64;

/// What a member stub says when the guest calls one.
///
/// # The question this answers that a marker cannot
///
/// A marker behind a field says *the guest read this member*. It cannot say what the guest
/// then **did** with it, because the moment the value is used as a function pointer the run
/// ends on an unmapped address and the arguments are gone.
///
/// A stub there answers harmlessly and says what it was called with - so a runtime that
/// takes a function pointer out of a structure it was handed, and calls it with a string, is
/// as legible as the entry point was when it called field zero with `sceKernelDlsym` (D375).
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

/// One table of stubs, mapped once, for every member of every stubbed field.
///
/// **One buffer rather than one page per stub.** A page each would be four megabytes of
/// mapping for a thousand stubs, and every one of them is the same thirty-two bytes with two
/// immediates changed.
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
        // Leaked deliberately, like every other stub here: the guest holds these addresses
        // for as long as it runs.
        let buffer = Box::leak(Box::new(
            crate::exec::ExecutableBuffer::new(&code).expect("the member stubs must be mappable"),
        ));
        buffer.address()
    })
}

/// Fills a mapped marker region so every word is a stub that names where it was read from.
///
/// The second level of [`fill_with_content_markers`]: that one makes a *read* legible, this
/// makes a **call** legible. A guest that reads a member and uses it as a pointer to data
/// still faults on an address inside this table, which the fault reporter names.
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
            // Past what is stubbed, a marker still names itself - which is the older, weaker
            // answer and better than nothing.
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

/// Names for the globals a run pointed at a gadget stub, by index.
///
/// Published rather than passed, for the same reason the data symbols are: a stub arrives on
/// a bare frame with no room for a context.
static GLOBAL_NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Publishes the names a gadget stub reports itself by.
pub fn install_global_names(names: Vec<String>) {
    let _ = GLOBAL_NAMES.set(names);
}

/// How many registers a gadget stub saves: `rax` and the six argument registers, plus `r10`.
pub const SAVED_GADGET_REGISTERS: usize = 8;

/// What a gadget stub says when the guest calls one.
///
/// # Why `rax` is the point
///
/// Every other diagnostic here reports the *arguments*, because everything else it watches is
/// a function. A syscall gadget is not a function: the number it is being asked to perform
/// arrives in `rax` on this architecture, which no argument-shaped report can see.
///
/// So this saves `rax` first and `r10` last - the register a `syscall` uses where an ordinary
/// call would use `rcx` - and prints all eight. Between them they say which convention the
/// caller used, which is the whole question (D377).
extern "sysv64" fn report_gadget_call(index: u64, saved: *const u64) -> u64 {
    use std::io::Write as _;

    let name = GLOBAL_NAMES
        .get()
        .and_then(|names| names.get(index as usize).cloned())
        .unwrap_or_else(|| format!("global #{index}"));
    // SAFETY: the stub that tail-called this wrote exactly `SAVED_GADGET_REGISTERS` words
    // into a buffer this crate leaked and owns for the life of the process.
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

/// The bytes of a syscall gadget: the registers a syscall reads, and the ones it keeps.
///
/// # The convention, which is not a function's
///
/// A guest reaches this by calling a pointer it keeps where a real system would keep
/// `syscall; ret`. So it arrives with the **number in `rax`** and its arguments in the
/// registers a `syscall` instruction reads: `rdi`, `rsi`, `rdx`, **`r10`**, `r8`, `r9`. The
/// fourth is `r10` rather than `rcx` because the instruction destroys `rcx`, and that one
/// difference is why a gadget cannot be watched by any argument-shaped stub (D377).
///
/// # What it must *not* destroy, which is the part that bites
///
/// `syscall` clobbers `rax`, `rcx` and `r11`, and **preserves everything else**. A Rust
/// function does not: the System V convention lets a callee destroy all six argument
/// registers. So a gadget that simply tail-called a dispatcher would hand the guest back its
/// own arguments as rubble - and the guest, having called what it believes is one instruction,
/// carries straight on using them.
///
/// That is not hypothetical. The first version tail-called, and `klog_printf` went on to pass
/// a destroyed register to `vsnprintf` as a `va_list` and read address `-1` (D378).
///
/// # The alignment, which is the guest's and cannot be assumed
///
/// Everything else here reaches this project through a call site a compiler wrote, so the
/// stack arrives sixteen-byte aligned and the run reports as much - *all on a conforming
/// stack*. **A gadget is not reached that way.** The guest holds a pointer where a real
/// system holds `syscall; ret` and goes through it however its own code happens to, and
/// `ftpsrv` arrives here eight off.
///
/// That is not a fault until the dispatcher's frame is built on it, and then it is a
/// spectacular one: the optimiser copies the six saved arguments with `movaps`, an aligned
/// SSE store, which raises a general-protection fault on a misaligned address. Windows
/// reports that as an access violation at **`0xffffffffffffffff`** - an address no program
/// computed, which read as a wild pointer and survived a dozen eliminations as one (D384).
///
/// So the gadget aligns the stack itself, keeps the old one in `rbp` - callee-saved in both
/// conventions, so the dispatcher gives it back - and restores it before returning.
///
/// ```text
/// mov r11, imm64      the save buffer
/// mov [r11+0],  rax   the number, then the six the convention passes
/// mov [r11+8],  rdi
/// mov [r11+16], rsi
/// mov [r11+24], rdx
/// mov [r11+32], r10
/// mov [r11+40], r8
/// mov [r11+48], r9
/// push rdi rsi rdx r8 r9 r10     what a syscall would have preserved
/// push rbp                       the guest's, and what the alignment is remembered in
/// mov rbp, rsp
/// and rsp, -16                   whatever the guest was on, this is aligned
/// mov rdi, imm64                 the buffer, as the dispatcher's only argument
/// mov r11, imm64
/// call r11                       a call, not a jump: there is unwinding to do
/// mov rsp, rbp                   back to the guest's stack, however it was aligned
/// pop rbp
/// pop r10 r9 r8 rdx rsi rdi      in reverse
/// ret                            with the answer in rax, as the instruction leaves it
/// ```
fn syscall_gadget_code(buffer: u64, dispatch: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(96);
    code.extend_from_slice(&[0x49, 0xBB]);
    code.extend_from_slice(&buffer.to_le_bytes());
    for (rex, modrm, disp) in [
        (0x49_u8, 0x43_u8, 0_u8), // rax - the number
        (0x49, 0x7B, 8),          // rdi
        (0x49, 0x73, 16),         // rsi
        (0x49, 0x53, 24),         // rdx
        (0x4D, 0x53, 32),         // r10, where a syscall's fourth argument lives
        (0x4D, 0x43, 40),         // r8
        (0x4D, 0x4B, 48),         // r9
    ] {
        code.extend_from_slice(&[rex, 0x89, modrm, disp]);
    }
    // push rdi / rsi / rdx / r8 / r9 / r10
    code.extend_from_slice(&[0x57, 0x56, 0x52]);
    code.extend_from_slice(&[0x41, 0x50, 0x41, 0x51, 0x41, 0x52]);
    // push rbp / mov rbp, rsp / and rsp, -16 - the guest's alignment is whatever it was, and
    // `and` makes this one right whichever it was. `rbp` is callee-saved in both conventions,
    // so the dispatcher hands it back and the guest's own value is restored below.
    code.extend_from_slice(&[0x55]);
    code.extend_from_slice(&[0x48, 0x89, 0xE5]);
    code.extend_from_slice(&[0x48, 0x83, 0xE4, 0xF0]);
    // mov rdi, imm64 / mov r11, imm64 / call r11
    code.extend_from_slice(&[0x48, 0xBF]);
    code.extend_from_slice(&buffer.to_le_bytes());
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
/// `dispatch` is what performs the call - passed in rather than named here, because the table
/// it dispatches through belongs to a crate this one does not depend on.
///
/// **One buffer, and that is a stated limit.** Two guest threads issuing syscalls at the same
/// instant would overwrite each other's registers. Nothing measured does it, the payloads
/// reaching this are single-threaded at the point they reach it, and a per-thread buffer is
/// the fix when something does.
pub fn syscall_gadget(dispatch: u64, saved_registers: usize) -> Option<u64> {
    use std::sync::OnceLock;
    static GADGET: OnceLock<Option<u64>> = OnceLock::new();
    *GADGET.get_or_init(|| {
        let saved: &'static mut [u64] = Box::leak(vec![0_u64; saved_registers].into());
        let code = syscall_gadget_code(saved.as_ptr() as u64, dispatch);
        let buffer = crate::exec::ExecutableBuffer::new(&code).ok()?;
        Some(Box::leak(Box::new(buffer)).address())
    })
}

/// One stub per global, each saving every register and naming itself.
///
/// # The encoding, which is written out because it has to be exact
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
/// The buffer is per stub rather than shared, so two guest threads calling two gadgets cannot
/// overwrite each other's report.
///
/// **Aligned rather than tail-jumped, for the reason the syscall gadget is** (D384): a guest
/// reaches a gadget through a pointer it keeps rather than a call site a compiler wrote, so
/// the stack it arrives on is whatever the guest was using - and a reporter whose frame the
/// optimiser fills with `movaps` faults on the store, not on anything it was watching.
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
    // push rbp / mov rbp, rsp / and rsp, -16 - the guest's alignment is not this project's
    // to assume, and `rbp` is callee-saved in both conventions, so the reporter hands it back.
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
    // mov rsp, rbp / pop rbp / ret - the reporter's answer is already in rax, which is what
    // the guest reads.
    code.extend_from_slice(&[0x48, 0x89, 0xEC]);
    code.extend_from_slice(&[0x5D]);
    code.push(0xC3);
    code
}

/// Bytes one gadget stub occupies, rounded so each starts on a sixteen-byte boundary.
const GADGET_STUB_LEN: usize = 96;

/// Addresses of `count` gadget stubs, mapped once.
///
/// Leaked deliberately: the guest holds these for as long as it runs, and a buffer freed
/// underneath it would turn a diagnostic into a use-after-free.
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

/// Where the markers *behind* a field start.
///
/// # The second depth, and why one was not enough
///
/// A field marker says the guest used field `n`. It cannot say anything more, because the
/// moment the guest reads *through* the field the marker has done its job and what comes back
/// is whatever is in the page - and a zeroed page answers zero, which names nothing.
///
/// So the page behind each field can hold markers of its own, one per word, each naming the
/// field it belongs to **and the offset it was read from**. A guest that reads a pointer out
/// of a structure it was handed then faults on a value that says which structure and which
/// member (D369).
///
/// A separate base from [`SENTINEL_BASE`] so the two depths can never be confused for one
/// another, and adjacent to it so both are recognisable on sight as ours.
pub const CONTENT_BASE: u64 = 0x0000_5E28_0000_0000;

/// How far apart consecutive fields' content markers sit.
///
/// **Deliberately not [`SENTINEL_STRIDE`], and the difference is the whole point.** A guest
/// that truncates a marker to thirty-two bits - which they do, because a structure member is
/// often an `int` - keeps only the low half, and with one stride the two depths produce the
/// *same* low half. The question they exist to tell apart, "did that number come from the
/// field or from what the field points at", would then have the same answer either way.
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
        // SAFETY: the caller guarantees the whole region is mapped writable, and `offset`
        // is inside it by construction. Written unaligned because nothing here promises the
        // caller's base is eight-byte aligned.
        unsafe {
            std::ptr::write_unaligned(std::ptr::with_exposed_provenance_mut::<u64>(at), value);
        }
    }
}

/// Reads a faulting address back as the field whose *contents* it came from.
///
/// The companion to [`sentinel_slot`], one level deeper: that one says the guest used a
/// field, this one says the guest read through a field and then used what it found, and says
/// from which offset.
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
/// [`None`] for any address outside the marker range, which is most of them: a fault that
/// has nothing to do with the block must not be reported as though it named a field.
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
/// The form a *thread* entry point needs. A guest thread body is called as
/// `void *start(void *arg)`, so the argument the guest handed to the create call has to
/// arrive in the first System V argument register rather than as whatever was left there.
///
/// # Safety
///
/// As [`enter_guest`], and additionally: `argument` is passed to guest code unexamined,
/// so if the guest dereferences it, it must be a valid guest address.
pub unsafe fn enter_guest_with_argument(entry: u64, stack_pointer: u64, argument: u64) -> u64 {
    // SAFETY: the caller's contract, unchanged - a second argument of zero is what `rsi`
    // already held for a one-argument callee, now stated rather than left to the clobber
    // list.
    unsafe { enter_guest_with_arguments(entry, stack_pointer, argument, 0) }
}

/// Transfers control to guest code on a dedicated stack, with two arguments.
///
/// The form `main(int argc, char **argv)` needs. Separate from the one-argument version
/// only in that `rsi` is stated as an input rather than a clobber - it was always being
/// set, to nothing in particular.
///
/// **Why this exists.** Entering a payload at its `main` rather than its declared entry
/// (D343) means the callee is an ordinary C function with the ordinary C signature, and
/// handing it a process-argument block as `argc` gives it a wild count to iterate. The
/// first thing both payloads measured do is parse their options.
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
    // SAFETY: the block saves the host stack pointer in `r12` - callee-saved under
    // System V, so the guest must restore it - switches to the guest stack, calls the
    // guest, and switches back before any compiler-generated code runs. Every register
    // the guest may destroy is declared clobbered, so the compiler preserves whatever
    // it needs across the call. The caller guarantees `entry` and `stack_pointer`.
    unsafe {
        core::arch::asm!(
            "mov r12, rsp",
            "mov rsp, {stack}",
            "call {entry}",
            "mov rsp, r12",
            stack = in(reg) stack_pointer,
            entry = in(reg) entry,
            out("rax") returned,
            // System V caller-saved integer registers. `rsi` and `rdi` are callee-saved
            // on Windows, which is exactly why they must be listed.
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
            // Every vector register. `xmm6`-`xmm15` are callee-saved on Windows and
            // caller-saved under System V - the disagreement this list settles.
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
/// The form a callback with an `InitOnce`-shaped signature needs - `(handle, parameter, context)`
/// in `rdi`, `rsi`, `rdx`. Separate from the two-argument version only in that `rdx` is stated as an
/// input rather than a clobber; it was always being set, to nothing in particular.
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
    // SAFETY: as `enter_guest_with_arguments`, with `rdx` additionally supplied as the third
    // argument rather than clobbered. The stack save/switch/restore and the full clobber list are
    // unchanged, and the caller guarantees `entry` and `stack_pointer`.
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
    /// **A marker decodes back to the slot it came from, and nothing else does.**
    ///
    /// The whole technique rests on this being exact: a fault address is read as a field
    /// number, and a decoder that accepted addresses outside the range would turn an
    /// unrelated crash into a confident statement about a structure nobody has seen.
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

    #[test]
    fn control_transfers_to_generated_code_on_a_guest_stack_and_comes_back() {
        // The whole mechanism end to end: real machine code, a real stack switch, and
        // a value carried back in `rax`. Nothing short of executing it proves the
        // register discipline above is right.
        let code = emit_return_constant(0x0BAD_C0DE);
        let buffer = ExecutableBuffer::new(&code).expect("map executable memory");
        let stack = GuestStack::reserve(TEST_STACK_BASE, 64 * 1024).expect("reserve a stack");

        // SAFETY: `buffer` holds mapped executable code that returns immediately, and
        // `stack` is a mapped, writable, sixteen-byte-aligned guest stack with a guard
        // page beneath it. Both outlive the call.
        let got = unsafe { enter_guest(buffer.address(), stack.initial_pointer()) };
        assert_eq!(got, 0x0BAD_C0DE, "the guest return value must survive");
    }

    #[test]
    fn an_argument_reaches_the_guest_in_the_first_register() {
        // A thread body is `void *start(void *arg)`, so this is the difference between
        // a guest thread getting its context and getting whatever was left in `rdi`.
        // Nothing short of executing it proves the register is right.
        //
        // `48 89 F8` is `mov rax, rdi`; `C3` is `ret`. Hand-encoded because this is the
        // only place that needs it and a two-instruction emitter would hide it.
        let code = [0x48, 0x89, 0xF8, 0xC3];
        let buffer = ExecutableBuffer::new(&code).expect("map executable memory");
        let stack =
            GuestStack::reserve(TEST_STACK_BASE + 0x200_0000, 64 * 1024).expect("reserve a stack");

        // SAFETY: mapped executable code that reads one register and returns, on a
        // mapped, aligned guest stack. The argument is never dereferenced.
        let got = unsafe {
            super::enter_guest_with_argument(buffer.address(), stack.initial_pointer(), 0xFEED)
        };
        assert_eq!(got, 0xFEED, "the argument must arrive in rdi");
    }

    /// **Three arguments reach guest code and their result comes back** - the basis for calling a
    /// callback like `call_once`'s initialiser, which arrives as `(handle, parameter, context)`. If
    /// the stack switch or any of the three argument registers were wrong, the sum would not be the
    /// sum.
    ///
    /// `48 89 F8` mov rax, rdi; `48 01 F0` add rax, rsi; `48 01 D0` add rax, rdx; `C3` ret. Three
    /// arguments in, their total out.
    #[test]
    fn three_arguments_reach_the_guest_and_their_result_returns() {
        let code = [0x48, 0x89, 0xF8, 0x48, 0x01, 0xF0, 0x48, 0x01, 0xD0, 0xC3];
        let buffer = ExecutableBuffer::new(&code).expect("map executable memory");
        let stack =
            GuestStack::reserve(TEST_STACK_BASE + 0x400_0000, 64 * 1024).expect("reserve a stack");
        // SAFETY: mapped executable code that reads three registers and returns their sum, on a
        // mapped, aligned guest stack; it dereferences none of the arguments.
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

    /// A reporting stub is what the shift-and-tail-call comment says it is.
    ///
    /// Asserted byte by byte, because an encoding that is wrong by one bit is a different
    /// valid instruction rather than an error - the same reason the thunk encoder is
    /// tested this way.
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

    /// Each stub carries its own slot, which is the whole reason there is one per slot.
    #[test]
    fn two_reporting_stubs_differ_only_in_the_slot_they_name() {
        let (first, second) = (super::reporting_stub(0), super::reporting_stub(1));
        assert_eq!(first[..11], second[..11], "the same shift");
        assert_ne!(first[11..19], second[11..19], "a different slot");
        assert_eq!(first[19..], second[19..], "the same reporter");
    }

    /// The known half of the handoff structure is in field zero and nowhere else.
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

    /// **The gadget reads what a syscall reads and keeps what it keeps** (D378).
    ///
    /// Asserted byte for byte. Two things here are the difference between working and
    /// silently corrupting a guest: the fourth argument comes from `r10` rather than `rcx`,
    /// and the six argument registers are pushed and popped, because `syscall` preserves them
    /// and a Rust call does not.
    #[test]
    fn a_syscall_gadget_reads_r10_and_preserves_what_a_syscall_preserves() {
        let code = super::syscall_gadget_code(0x1000, 0x2000);
        assert_eq!(&code[..2], &[0x49, 0xBB], "mov r11, imm64");
        assert_eq!(&code[2..10], &0x1000_u64.to_le_bytes(), "the save buffer");

        let stores = &code[10..38];
        assert_eq!(&stores[0..4], &[0x49, 0x89, 0x43, 0], "mov [r11+0], rax");
        assert_eq!(
            &stores[16..20],
            &[0x4D, 0x89, 0x53, 32],
            "mov [r11+32], r10 - the fourth argument, not rcx"
        );

        assert_eq!(
            &code[38..47],
            &[0x57, 0x56, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52],
            "push rdi rsi rdx r8 r9 r10 - what a syscall would have preserved"
        );
        // **The alignment, which is the whole of D384.** The guest reaches a gadget through a
        // pointer it keeps rather than a call site a compiler wrote, so the stack it arrives
        // on is whatever the guest was using - `ftpsrv` arrives eight off, and the
        // dispatcher's `movaps` then faults on its own frame.
        assert_eq!(code[47], 0x55, "push rbp - the guest's, and the old rsp");
        assert_eq!(&code[48..51], &[0x48, 0x89, 0xE5], "mov rbp, rsp");
        assert_eq!(
            &code[51..55],
            &[0x48, 0x83, 0xE4, 0xF0],
            "and rsp, -16 - aligned whatever it was"
        );
        assert_eq!(&code[55..57], &[0x48, 0xBF], "mov rdi, imm64");
        assert_eq!(&code[65..67], &[0x49, 0xBB], "mov r11, imm64");
        assert_eq!(&code[75..78], &[0x41, 0xFF, 0xD3], "call r11, not a jump");
        assert_eq!(&code[78..81], &[0x48, 0x89, 0xEC], "mov rsp, rbp");
        assert_eq!(code[81], 0x5D, "pop rbp - the guest's own value back");
        assert_eq!(
            &code[82..91],
            &[0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x5E, 0x5F],
            "pop r10 r9 r8 rdx rsi rdi - in reverse"
        );
        assert_eq!(
            code[91], 0xC3,
            "ret, with the answer where the instruction leaves it"
        );
    }

    /// **The stack the dispatcher is entered on is aligned however the guest arrived.**
    ///
    /// The property, rather than the bytes: whatever `rsp` was at the gadget's first
    /// instruction, `and rsp, -16` makes it sixteen-byte aligned before the `call` puts a
    /// return address on it - which is what the convention requires and what the optimiser's
    /// `movaps` stores depend on (D384).
    #[test]
    fn the_gadget_aligns_whatever_stack_it_was_entered_on() {
        let code = super::syscall_gadget_code(0x1000, 0x2000);
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

    /// The pushes and the pops must match, or the guest returns to rubble.
    #[test]
    fn the_gadget_restores_exactly_what_it_saved() {
        let code = super::syscall_gadget_code(0x1000, 0x2000);
        let pushes = &code[38..47];
        let pops = &code[82..91];
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

    /// **The saves are what a syscall gadget needs and a call report cannot give** (D377).
    ///
    /// Asserted byte for byte, because an encoding wrong by one bit is a plausible different
    /// instruction rather than an error - and here the difference between saving `rax` and
    /// saving something else is the difference between seeing a syscall number and not.
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

        // Aligned first, for the reason the syscall gadget is (D384): a guest reaches this
        // through a pointer it keeps, so the stack it arrives on is its own.
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

    /// **The two depths must not collide in the low half** (D369).
    ///
    /// A guest truncating a marker to thirty-two bits - which they do, because a structure
    /// member is often an `int` - keeps only the low half. If both depths produced the same
    /// low half, the question they exist to tell apart would have the same answer either way.
    /// **A named field wins over whatever the block would have put there** (D375).
    ///
    /// The sweep this exists for names one field and leaves the rest to the markers, so a
    /// named value that lost to the default would make every run of the sweep identical.
    #[test]
    fn a_named_field_replaces_what_the_block_would_have_held() {
        // A separate block from the one the other test builds, because the real one is
        // built once per process - so this asserts the composition rule on the same call
        // the worker makes, with the field values it would pass.
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

    /// An address that is neither names nothing, which is most of them.
    #[test]
    fn an_address_belonging_to_neither_depth_names_nothing() {
        assert_eq!(super::content_slot(0x4000_0000_0000), None);
        assert_eq!(super::sentinel_slot(0x4000_0000_0000), None);
    }

    /// Filling writes a word per slot, each naming the field and the offset it sits at.
    #[test]
    fn filling_makes_every_word_name_where_it_came_from() {
        // Two fields' worth, which is enough to show the stride and the offset apart.
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

    #[test]
    fn the_host_stack_is_intact_afterwards() {
        // If `r12` were not restored, or the guest stack leaked into host frames, this
        // would corrupt locals rather than fail cleanly - so it is worth asserting that
        // ordinary code still works either side of the transfer.
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
