# 2026-09-01 - (/loop) Execute breakpoints: capture a guest value where it is used

The user asked to build the watchpoint feature and use it to crack the walls. Found orbistoun already has
hardware *data* watchpoints (arms DR0-3, traps through the #DB handler; D276). Added a third kind, Execute
(`<addr>:x`): a one-shot instruction breakpoint (R/W=00, LEN=00) that on its first hit snapshots all sixteen
registers into statics (allocation-free) and self-disarms by clearing its Dr7 enable bit in the exception
context - so the instruction runs and the guest continues, no resume-flag/single-step dance. The summary
prints the register state the instruction was entered with (i.e. a function's arguments). Provenance-clean
(reads registers, not guest code, per D277). Recorded D458.

Verified: armed at image+0xafcc08 it fires once, prints the registers, disarms, and the guest runs on
(`hit 1 time(s); ... rax=0x0 ... rbx=0x4000 ... r14=0x20 ...`). fmt/clippy/tests pass for the additions (44);
the pre-existing `enter` too_many_lines in lib.rs is untouched debt.

And the tool then corrected its own first guess, which is the point. The snapshot suggested r14=0x20 (below
tlsf's 0x28 minimum) was the rejected pool size. Reading the instruction bytes the report captures before
0xafcc08 killed that: the faulting insn is `vmovdqu [r12], xmm0`, and r12 is EXPLICITLY zeroed (`xor r12d,r12d`)
two insns earlier, on the branch a `je` takes when the preceding `call [rax+0x10]` (an indirect/virtual call)
returns 0. So 0xafcc08 is a virtual call returning null, not the tlsf size path; r14=0x20 was a coincidence.
Next: an execute breakpoint just before 0xafcc08 to capture rax, read [rax+0x10] for the method address, break
there to see why it answers zero. D450 non-determinism applies (fires only on runs taking its branch).
