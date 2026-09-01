# Phase 0b - ABI spike *(DONE, both platforms)*


A few hundred bytes of hand-assembled machine code - ours, not anyone else's - that
calls one imported function. Prove the thunk mechanism end to end: the guest calls
the host, arguments arrive in the right registers, the return value lands, and the
call site is recoverable.

**Why early.** As the chain is ordered, guest code first executes at phase 4, by
which point the parser, symbol loading, and address space are built on an unvalidated
assumption. If the calling convention, stack alignment, or TLS layout is wrong, phase
4 is an expensive place to discover it.

Keep it crude - plain RWX allocation, no fixed addresses, no loader. It is a spike:
throw it away, or grow it into phase 4.

**Answered: yes** (D056). Hand-assembled machine code calls a host function with all
six arguments intact, on Windows and Linux.

The finding that justified doing it early: host functions must be `extern "sysv64"`,
because on Windows the native convention differs and a mismatch would be **silent** -
the callee reads whatever was in `rcx`. Kept as `orbistoun-abi` rather than thrown
away; it grows into phase 4's thunk mechanism.

