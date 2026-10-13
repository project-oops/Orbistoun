//! `setjmp` and `longjmp` as leaves (D734): called straight from the import stub, so the guest's
//! callee-saved registers are still in the registers when `setjmp` runs, and `longjmp` can put them
//! back and resume the guest where its `setjmp` returned.
//!
//! The buffer follows FreeBSD amd64's `_setjmp` (`lib/libc/amd64/gen/_setjmp.S`): the resume address
//! at 0, `rbx` at 8, the stack pointer at 16, `rbp` at 24, `r12`-`r15` at 32-56, the x87 control word
//! at 64 and `MXCSR` at 68. Only this file's `longjmp` reads it back, and the guest's own code only
//! needs the registers to be in it: an IL2CPP title's collector calls `setjmp` to spill its
//! callee-saved registers onto the stack before it scans the stack for references, hundreds of
//! thousands of times a run. The stack pointer kept is the guest's after `setjmp` returns, so
//! `longjmp` resumes by jumping rather than by returning through a slot.
//!
//! [`JMP_MAGIC`] at 72, inside FreeBSD's 96-byte `jmp_buf`, marks a buffer this `setjmp` filled. A
//! `longjmp` into anything else stops at `ud2` rather than jumping through whatever it holds.
//!
//! Each leaf runs under `orbistoun_thunk`'s `leaf_entry`, which pushed the guest's `rbp` and keeps
//! its own `rsp` in `rbp`: so `[rbp]` is the guest's `rbp`, `[rbp + 8]` the guest's return address
//! and `rbp + 16` the guest's stack pointer once its call returns. That frame is the contract these
//! read; on the traced path, when a diagnostic names either import, [`traced_setjmp`] saves nothing
//! and [`traced_longjmp`] answers the placeholder.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};

/// Written at `jmp_buf + 72` by [`setjmp`], checked by [`longjmp`]: "OBSJMPBF".
pub const JMP_MAGIC: u64 = 0x4642_504d_4a53_424f;

/// `setjmp(env)`: saves the guest's callee-saved registers, stack pointer, return address and
/// floating-point controls into `env`, and answers 0, the direct return.
#[unsafe(naked)]
pub extern "sysv64" fn setjmp(_: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    core::arch::naked_asm!(
        "mov rax, qword ptr [rbp + 8]",
        "mov qword ptr [rdi], rax",
        "mov qword ptr [rdi + 8], rbx",
        "lea rax, [rbp + 16]",
        "mov qword ptr [rdi + 16], rax",
        "mov rax, qword ptr [rbp]",
        "mov qword ptr [rdi + 24], rax",
        "mov qword ptr [rdi + 32], r12",
        "mov qword ptr [rdi + 40], r13",
        "mov qword ptr [rdi + 48], r14",
        "mov qword ptr [rdi + 56], r15",
        "fnstcw word ptr [rdi + 64]",
        "stmxcsr dword ptr [rdi + 68]",
        "mov rax, {magic}",
        "mov qword ptr [rdi + 72], rax",
        "xor eax, eax",
        "ret",
        magic = const JMP_MAGIC,
    )
}

/// `longjmp(env, value)`: restores what [`setjmp`] saved in `env` and resumes the guest where that
/// `setjmp` returned, answering `value` there, or 1 for a `value` of 0, as C requires. Never
/// returns to its caller.
#[unsafe(naked)]
pub extern "sysv64" fn longjmp(_: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    core::arch::naked_asm!(
        "mov rax, {magic}",
        "cmp qword ptr [rdi + 72], rax",
        "jne 2f",
        "mov eax, esi",
        "test eax, eax",
        "jnz 3f",
        "mov eax, 1",
        "3:",
        "mov rbx, qword ptr [rdi + 8]",
        "mov rsp, qword ptr [rdi + 16]",
        "mov rbp, qword ptr [rdi + 24]",
        "mov r12, qword ptr [rdi + 32]",
        "mov r13, qword ptr [rdi + 40]",
        "mov r14, qword ptr [rdi + 48]",
        "mov r15, qword ptr [rdi + 56]",
        "fldcw word ptr [rdi + 64]",
        "ldmxcsr dword ptr [rdi + 68]",
        "jmp qword ptr [rdi]",
        "2:",
        "ud2",
        magic = const JMP_MAGIC,
    )
}

/// `setjmp` on the traced path, when a diagnostic names it: 0, the direct return, with nothing
/// saved - the trampoline has replaced the guest's registers by the time this runs.
pub(crate) fn traced_setjmp(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    0
}

/// `longjmp` on the traced path: the placeholder, since it cannot resume the guest from here.
pub(crate) fn traced_longjmp(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    u64::from(GuestError::Unimplemented.as_raw())
}

/// The leaves this module provides, by symbol name.
#[must_use]
pub fn leaves() -> &'static [(&'static str, orbistoun_core::LeafFn)] {
    &[("setjmp", setjmp), ("longjmp", longjmp)]
}
