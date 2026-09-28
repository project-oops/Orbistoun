//! Execution contexts that take turns on one host thread: the machinery under user-mode fibers.
//!
//! A fiber is guest code running on memory the guest supplied as its stack, suspended and resumed
//! by explicit calls rather than by a scheduler. Suspending one is a System V function call that
//! returns later, possibly on another host thread: [`switch`] saves what a callee must preserve
//! onto the current stack, records the stack pointer, and resumes another context by the reverse
//! steps. Everything a caller may lose across a call is lost, so nothing else is saved.
//!
//! A suspended context's stack, from its saved stack pointer upward, holds [`SAVED_WORDS`] words:
//!
//! ```text
//! +0   MXCSR (low half) and the x87 control word (bits 32..48)
//! +8   r15
//! +16  r14
//! +24  r13
//! +32  r12
//! +40  rbx
//! +48  rbp
//! +56  where it resumes: the return address of its `switch` call
//! ```
//!
//! A context that has never run is given the same shape by [`initial_frame`], resuming at a start
//! routine that calls the entry function, so the first switch into it needs no special case.

/// Words a suspended context keeps on its own stack.
pub const SAVED_WORDS: usize = 8;

/// Alignment of a fresh context's stack top, and so of its stack pointer on entry to the start
/// routine: System V requires sixteen bytes before a `call`.
pub const CONTEXT_ALIGN: u64 = 16;

/// What a context that has never run starts by doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    /// The function it calls, `entry(argument, value)`, where `value` is what the first
    /// [`switch`] into it passes.
    pub entry: u64,
    /// The first argument, fixed when the context is built.
    pub argument: u64,
    /// A function called as `on_return(identity)` if `entry` ever returns. It must not return:
    /// there is no caller below the entry to return to.
    pub on_return: u64,
    /// Passed to `on_return`, so it can say which context ended.
    pub identity: u64,
    /// The float control state the context begins with, as [`float_control`] reads it.
    pub float_control: u64,
}

/// Lays out a context that has never run, below `top`.
///
/// Answers the stack pointer to resume it at and the [`SAVED_WORDS`] words to write from there
/// upward. `top` is aligned down to [`CONTEXT_ALIGN`] first, so the start routine runs with the
/// alignment a caller would have given it. [`None`] when `top` is too low to hold the frame.
#[must_use]
pub fn initial_frame(top: u64, start: &Start) -> Option<(u64, [u64; SAVED_WORDS])> {
    let aligned = top / CONTEXT_ALIGN * CONTEXT_ALIGN;
    let pointer = aligned.checked_sub(SAVED_WORDS as u64 * 8)?;
    if pointer == 0 {
        return None;
    }
    let words = [
        start.float_control,
        start.identity,
        start.on_return,
        start.argument,
        start.entry,
        // rbx and rbp: nothing to carry, and a zero `rbp` ends a frame-pointer walk here.
        0,
        0,
        start_routine_address(),
    ];
    Some((pointer, words))
}

/// The address a fresh context resumes at.
#[must_use]
pub fn start_routine_address() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let routine: unsafe extern "sysv64" fn() = start_routine;
        routine as usize as u64
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

/// Suspends the running context and resumes another.
///
/// Writes the running context's stack pointer to `save`, then resumes the context whose stack
/// pointer is `target`, handing it `value`: as the return value of the [`switch`] call it is
/// suspended in, or, for a context built by [`initial_frame`], as the entry function's second
/// argument. Returns when something switches back to the context that called it, with the value
/// that switch passed.
///
/// Preserves exactly what System V makes a callee preserve: `rbx`, `rbp`, `r12`-`r15`, `rsp`, and
/// the control bits of `MXCSR` and the x87 control word. The whole `MXCSR` travels, status flags
/// included, since the flags describe the context's own arithmetic.
///
/// # Safety
///
/// - `save` must be writable, and stay so until the write, which is the last thing done on the
///   running context's stack.
/// - `target` must be a stack pointer this function wrote through a `save`, whose context has not
///   been resumed since, or one [`initial_frame`] answered whose words were written there. Either
///   way the memory it points into must be live and belong to no running context.
/// - Resuming a context resumes every frame on its stack, host or guest, so each must still be
///   valid to return into: nothing they borrow may have been freed or moved while it was suspended.
#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
pub unsafe extern "sysv64" fn switch(save: *mut u64, target: u64, value: u64) -> u64 {
    core::arch::naked_asm!(
        // What the callee preserves, pushed so the saved stack pointer finds them.
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // One word for the float control state: MXCSR in its low half, the x87 control word above.
        "sub rsp, 8",
        "stmxcsr [rsp]",
        "fnstcw [rsp + 4]",
        // The last write on this stack. Once it lands, another thread may resume this context.
        "mov [rdi], rsp",
        // The other context's stack, and its state back in the reverse order.
        "mov rsp, rsi",
        "ldmxcsr [rsp]",
        "fldcw [rsp + 4]",
        "add rsp, 8",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        // Its `switch` call returns this; a fresh context's start routine reads it from here.
        "mov rax, rdx",
        "ret",
    )
}

/// Suspends the running context and resumes another.
///
/// # Safety
///
/// See the x86-64 documentation; this build cannot do it at all.
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn switch(_save: *mut u64, _target: u64, _value: u64) -> u64 {
    unimplemented!("switching guest contexts is x86-64 only")
}

/// Where a context built by [`initial_frame`] begins, entered by [`switch`]'s `ret`.
///
/// Arrives with `rsp` at the aligned top, the entry in `r12`, its first argument in `r13`, the
/// return hook in `r14`, the identity in `r15`, and the value the switch passed in `rax`. The two
/// calls are made with the stack sixteen-byte aligned, as System V requires, and a zero `rbp` ends
/// a frame-pointer walk at the entry function.
///
/// ```text
/// mov rdi, r13     entry(argument,
/// mov rsi, rax           value)
/// xor ebp, ebp
/// call r12
/// mov rdi, r15     on_return(identity), which never comes back
/// call r14
/// ud2              and if it does, stop here rather than run on
/// ```
#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
unsafe extern "sysv64" fn start_routine() {
    core::arch::naked_asm!(
        "mov rdi, r13",
        "mov rsi, rax",
        "xor ebp, ebp",
        "call r12",
        "mov rdi, r15",
        "call r14",
        "ud2",
    )
}

/// The calling thread's float control state, packed as a suspended context stores it: `MXCSR` in
/// the low half and the x87 control word in bits 32..48.
#[cfg(target_arch = "x86_64")]
#[must_use]
pub fn float_control() -> u64 {
    let mut word = 0_u64;
    // SAFETY: `stmxcsr` writes four bytes at the operand and `fnstcw` two bytes four further on,
    // all inside `word`, a live local nothing else aliases. Neither changes any other state.
    unsafe {
        core::arch::asm!(
            "stmxcsr [{at}]",
            "fnstcw [{at} + 4]",
            at = in(reg) &raw mut word,
            options(nostack, preserves_flags),
        );
    }
    word
}

/// The calling thread's float control state, where there is one.
#[cfg(not(target_arch = "x86_64"))]
#[must_use]
pub fn float_control() -> u64 {
    0
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::{SAVED_WORDS, Start, float_control, initial_frame, switch};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A stack for a test context: host memory, whose top `initial_frame` aligns.
    struct Stack(Vec<u64>);

    impl Stack {
        fn new() -> Self {
            Self(vec![0; 8 * 1024])
        }

        fn top(&self) -> u64 {
            self.0.as_ptr() as u64 + (self.0.len() * 8) as u64
        }
    }

    /// Lays out a fresh context on `stack` and answers its stack pointer.
    fn fresh(stack: &mut Stack, entry: u64, argument: u64, identity: u64) -> u64 {
        let start = Start {
            entry,
            argument,
            on_return: returned as *const () as usize as u64,
            identity,
            float_control: float_control(),
        };
        let (pointer, words) = initial_frame(stack.top(), &start).expect("the stack holds a frame");
        let first = usize::try_from((pointer - stack.0.as_ptr() as u64) / 8).expect("fits");
        stack.0[first..first + SAVED_WORDS].copy_from_slice(&words);
        pointer
    }

    /// Where each test's thread side is suspended, and where the hook below returns to.
    ///
    /// One per test, so tests running on parallel threads cannot resume each other.
    struct Rendezvous {
        thread: AtomicU64,
        fiber: AtomicU64,
        other: AtomicU64,
        first: AtomicU64,
        second: AtomicU64,
        third: AtomicU64,
    }

    impl Rendezvous {
        const fn new() -> Self {
            Self {
                thread: AtomicU64::new(0),
                fiber: AtomicU64::new(0),
                other: AtomicU64::new(0),
                first: AtomicU64::new(0),
                second: AtomicU64::new(0),
                third: AtomicU64::new(0),
            }
        }
    }

    /// The return hook every test context is built with: records which context ended in `third`
    /// and switches back to the thread side for good. `identity` is the rendezvous's address.
    extern "sysv64" fn returned(identity: u64) -> ! {
        // SAFETY: every test builds its contexts with its own static rendezvous as the identity.
        let meeting = unsafe { &*(identity as usize as *const Rendezvous) };
        meeting.third.store(identity, Ordering::SeqCst);
        let mut abandoned = 0_u64;
        // SAFETY: the thread side is suspended in `switch` and wrote `thread` before it was; nothing
        // resumes this context again, so `abandoned` need only stay writable for the write.
        unsafe { switch(&raw mut abandoned, meeting.thread.load(Ordering::SeqCst), 0) };
        unreachable!("an ended context is never resumed")
    }

    static FIRST_RUN: Rendezvous = Rendezvous::new();

    extern "sysv64" fn record_and_return(argument: u64, value: u64) {
        FIRST_RUN.first.store(argument, Ordering::SeqCst);
        FIRST_RUN.second.store(value, Ordering::SeqCst);
        // SAFETY: the thread side wrote `thread` in the switch that started this context.
        unsafe {
            switch(
                FIRST_RUN.fiber.as_ptr(),
                FIRST_RUN.thread.load(Ordering::SeqCst),
                0x7788,
            )
        };
    }

    /// A fresh context's entry receives both arguments, and the value it switches back with is
    /// what the thread side's switch returns.
    #[test]
    fn a_fresh_context_receives_both_arguments_and_hands_a_value_back() {
        let mut stack = Stack::new();
        let entry = record_and_return as *const () as usize as u64;
        let identity = &raw const FIRST_RUN as u64;
        let pointer = fresh(&mut stack, entry, 0x1122, identity);

        // SAFETY: `pointer` was laid out by `initial_frame` on `stack`, which outlives the call and
        // runs nothing else.
        let back = unsafe { switch(FIRST_RUN.thread.as_ptr(), pointer, 0x3344) };

        assert_eq!(
            FIRST_RUN.first.load(Ordering::SeqCst),
            0x1122,
            "the argument fixed at build"
        );
        assert_eq!(
            FIRST_RUN.second.load(Ordering::SeqCst),
            0x3344,
            "the value the switch passed"
        );
        assert_eq!(back, 0x7788, "and what the context switched back with");
    }

    static RESUMED: Rendezvous = Rendezvous::new();

    extern "sysv64" fn keep_a_local_across_switches(argument: u64, value: u64) {
        // On this context's own stack, read back after every resume.
        let local = std::hint::black_box([argument, value, argument ^ value, 0x5A5A]);
        // SAFETY: the thread side wrote `thread` in the switch that resumed this context.
        let second = unsafe {
            switch(
                RESUMED.fiber.as_ptr(),
                RESUMED.thread.load(Ordering::SeqCst),
                1,
            )
        };
        let seen = std::hint::black_box(local);
        RESUMED.first.store(
            u64::from(seen == [argument, value, argument ^ value, 0x5A5A]),
            Ordering::SeqCst,
        );
        RESUMED.second.store(second, Ordering::SeqCst);
        // SAFETY: as above.
        let third = unsafe {
            switch(
                RESUMED.fiber.as_ptr(),
                RESUMED.thread.load(Ordering::SeqCst),
                2,
            )
        };
        RESUMED.third.store(third, Ordering::SeqCst);
        // SAFETY: as above.
        unsafe {
            switch(
                RESUMED.fiber.as_ptr(),
                RESUMED.thread.load(Ordering::SeqCst),
                3,
            )
        };
    }

    /// A context switched away from resumes where it left off, with its stack intact, however
    /// many times it is resumed.
    #[test]
    fn a_suspended_context_resumes_where_it_left_off_with_its_stack_intact() {
        let mut stack = Stack::new();
        let entry = keep_a_local_across_switches as *const () as usize as u64;
        let pointer = fresh(&mut stack, entry, 0xAAAA, &raw const RESUMED as u64);

        // SAFETY: a fresh context on `stack`, which outlives every switch below.
        let first = unsafe { switch(RESUMED.thread.as_ptr(), pointer, 0xBBBB) };
        assert_eq!(first, 1, "the first switch back");

        // SAFETY: `fiber` is where that context saved itself, and nothing resumed it since.
        let second = unsafe {
            switch(
                RESUMED.thread.as_ptr(),
                RESUMED.fiber.load(Ordering::SeqCst),
                0xCCCC,
            )
        };
        assert_eq!(second, 2, "resumed after its first switch, not restarted");
        assert_eq!(
            RESUMED.first.load(Ordering::SeqCst),
            1,
            "its local survived the suspension"
        );
        assert_eq!(
            RESUMED.second.load(Ordering::SeqCst),
            0xCCCC,
            "and it got the new value"
        );

        // SAFETY: as above.
        let third = unsafe {
            switch(
                RESUMED.thread.as_ptr(),
                RESUMED.fiber.load(Ordering::SeqCst),
                0xDDDD,
            )
        };
        assert_eq!(third, 3);
        assert_eq!(
            RESUMED.third.load(Ordering::SeqCst),
            0xDDDD,
            "every resume delivers its value"
        );
    }

    static PAIR: Rendezvous = Rendezvous::new();

    extern "sysv64" fn hand_over_to_the_other(_argument: u64, value: u64) {
        PAIR.first.store(value, Ordering::SeqCst);
        // Straight to the second context, never through the thread side.
        // SAFETY: `other` holds the second context's fresh frame, written before this one started.
        let back = unsafe {
            switch(
                PAIR.fiber.as_ptr(),
                PAIR.other.load(Ordering::SeqCst),
                0x0B0B,
            )
        };
        PAIR.third.store(back, Ordering::SeqCst);
        // SAFETY: the thread side is suspended at `thread`.
        unsafe {
            switch(
                PAIR.fiber.as_ptr(),
                PAIR.thread.load(Ordering::SeqCst),
                0xE0E0,
            )
        };
    }

    extern "sysv64" fn report_and_hand_back(_argument: u64, value: u64) {
        PAIR.second.store(value, Ordering::SeqCst);
        let mut parked = 0_u64;
        // Back to the first, which resumes inside its own switch.
        // SAFETY: the first context saved itself at `fiber` when it switched here.
        unsafe { switch(&raw mut parked, PAIR.fiber.load(Ordering::SeqCst), 0x0C0C) };
    }

    /// One context switches directly to another and is resumed by it, with the thread side
    /// suspended throughout.
    #[test]
    fn one_context_switches_directly_to_another_and_back() {
        let mut first = Stack::new();
        let mut second = Stack::new();
        let identity = &raw const PAIR as u64;
        let a = fresh(
            &mut first,
            hand_over_to_the_other as *const () as usize as u64,
            0,
            identity,
        );
        let b = fresh(
            &mut second,
            report_and_hand_back as *const () as usize as u64,
            0,
            identity,
        );
        PAIR.other.store(b, Ordering::SeqCst);

        // SAFETY: two fresh contexts on stacks that outlive the switch.
        let back = unsafe { switch(PAIR.thread.as_ptr(), a, 0x0A0A) };

        assert_eq!(PAIR.first.load(Ordering::SeqCst), 0x0A0A, "the first ran");
        assert_eq!(
            PAIR.second.load(Ordering::SeqCst),
            0x0B0B,
            "and handed the second its value"
        );
        assert_eq!(
            PAIR.third.load(Ordering::SeqCst),
            0x0C0C,
            "which handed the first its own"
        );
        assert_eq!(back, 0xE0E0, "before the first came back to the thread");
    }

    static ENDED: Rendezvous = Rendezvous::new();

    extern "sysv64" fn just_return(_argument: u64, value: u64) {
        ENDED.first.store(value, Ordering::SeqCst);
    }

    /// An entry that returns reaches the return hook with its context's identity, rather than
    /// running off the top of its stack.
    #[test]
    fn an_entry_that_returns_reaches_the_hook_with_its_identity() {
        let mut stack = Stack::new();
        let identity = &raw const ENDED as u64;
        let pointer = fresh(
            &mut stack,
            just_return as *const () as usize as u64,
            0,
            identity,
        );

        // SAFETY: a fresh context on `stack`, which outlives the switch.
        unsafe { switch(ENDED.thread.as_ptr(), pointer, 0x99) };

        assert_eq!(ENDED.first.load(Ordering::SeqCst), 0x99, "the entry ran");
        assert_eq!(
            ENDED.third.load(Ordering::SeqCst),
            identity,
            "and the hook was told which ended"
        );
    }

    static CLOBBER: Rendezvous = Rendezvous::new();

    /// An `MXCSR` value no test thread uses: round toward zero, everything masked.
    static STRANGE_MXCSR: u32 = 0x7F80;
    /// An x87 control word no test thread uses: single precision, everything masked.
    static STRANGE_FPU_CONTROL: u16 = 0x007F;

    /// A context body that destroys every register a callee must preserve, and the float control
    /// state, then switches back without restoring any of them.
    #[unsafe(naked)]
    extern "sysv64" fn clobber_everything(_argument: u64, _value: u64) {
        core::arch::naked_asm!(
            "mov rbx, 0x0BAD0001",
            "mov rbp, 0x0BAD0002",
            "mov r12, 0x0BAD0003",
            "mov r13, 0x0BAD0004",
            "mov r14, 0x0BAD0005",
            "mov r15, 0x0BAD0006",
            "ldmxcsr [rip + {mxcsr}]",
            "fldcw [rip + {fpu}]",
            "lea rdi, [rip + {save}]",
            "mov rsi, [rip + {target}]",
            "xor edx, edx",
            "call {switch}",
            "ud2",
            mxcsr = sym STRANGE_MXCSR,
            fpu = sym STRANGE_FPU_CONTROL,
            save = sym CLOBBER_FIBER,
            target = sym CLOBBER_THREAD,
            switch = sym switch,
        )
    }

    static CLOBBER_FIBER: AtomicU64 = AtomicU64::new(0);
    static CLOBBER_THREAD: AtomicU64 = AtomicU64::new(0);

    /// Every register System V makes a callee preserve, and the float control state, come back
    /// from a switch as they went in, however the other context left them.
    #[test]
    fn callee_saved_registers_and_float_control_survive_a_switch() {
        let mut stack = Stack::new();
        let pointer = fresh(
            &mut stack,
            clobber_everything as *const () as usize as u64,
            0,
            &raw const CLOBBER as u64,
        );
        let before = float_control();
        let mut after = [0_u64; 6];
        // SAFETY: the block keeps `rbx` and `rbp`, which cannot be operands, on its own stack and
        // puts them back; it loads known values into the six preserved registers, switches into a
        // fresh context on `stack` that outlives the call, and stores the six on return into
        // `after`, a live local. Every register the switch may destroy under System V is declared.
        unsafe {
            core::arch::asm!(
                "push rbx",
                "push rbp",
                "push rcx",
                "sub rsp, 8",
                "mov rbx, 0x5AFE0001",
                "mov rbp, 0x5AFE0002",
                "mov r12, 0x5AFE0003",
                "mov r13, 0x5AFE0004",
                "mov r14, 0x5AFE0005",
                "mov r15, 0x5AFE0006",
                "call {switch}",
                "mov rcx, [rsp + 8]",
                "mov [rcx], rbx",
                "mov [rcx + 8], rbp",
                "mov [rcx + 16], r12",
                "mov [rcx + 24], r13",
                "mov [rcx + 32], r14",
                "mov [rcx + 40], r15",
                "add rsp, 16",
                "pop rbp",
                "pop rbx",
                switch = sym switch,
                in("rdi") CLOBBER_THREAD.as_ptr(),
                in("rsi") pointer,
                in("rdx") 0_u64,
                in("rcx") &raw mut after,
                out("r12") _,
                out("r13") _,
                out("r14") _,
                out("r15") _,
                clobber_abi("sysv64"),
            );
        }
        let returned_with = float_control();
        assert_eq!(
            after,
            [
                0x5AFE_0001,
                0x5AFE_0002,
                0x5AFE_0003,
                0x5AFE_0004,
                0x5AFE_0005,
                0x5AFE_0006
            ],
            "rbx rbp r12 r13 r14 r15, in that order"
        );
        assert_eq!(
            returned_with, before,
            "MXCSR and the x87 control word as they were, not as the other context left them"
        );
    }

    /// A frame is laid out below an aligned top, resuming at the start routine with the entry and
    /// its argument where the start routine reads them.
    #[test]
    fn a_fresh_frame_sits_below_an_aligned_top_and_resumes_at_the_start_routine() {
        let start = Start {
            entry: 0xE,
            argument: 0xA,
            on_return: 0x0,
            identity: 0x1D,
            float_control: 0x1F80,
        };
        let (pointer, words) = initial_frame(0x1_0009, &start).expect("room");
        assert_eq!(
            pointer,
            0x1_0000 - SAVED_WORDS as u64 * 8,
            "aligned down first"
        );
        assert_eq!(words[0], 0x1F80, "float control at the bottom");
        assert_eq!(words[1], 0x1D, "the identity in r15");
        assert_eq!(words[3], 0xA, "the argument in r13");
        assert_eq!(words[4], 0xE, "the entry in r12");
        assert_eq!(
            words[7],
            super::start_routine_address(),
            "resuming at the start routine"
        );
        assert_eq!(initial_frame(0x40, &start), None, "too low to hold a frame");
    }
}
