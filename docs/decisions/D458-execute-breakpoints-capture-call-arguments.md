# D458 - Execute breakpoints, so a guest-computed value can be read where it is used


**measured** - 2026-09-01 (user-directed /loop: build the watchpoint feature and use it to crack the walls)

Every remaining wall is a guest-computed value that was wrong and unchecked - the size a guest hands
`tlsf_add_pool` (D449, three titles), a base that was zero (D457). None can be read out of the load-transformed
eboots by disassembly, and this project refuses to disassemble the guest anyway (D277). The register dump
names *which* value was wrong (D457); this answers *what it was where it was used*.

**The feature.** `orbistoun-worker`'s watchpoints already arm x86 debug registers and trap through the #DB
handler for *data* accesses (D276). This adds a third kind, `Execute` (spelled `<addr>:x` in
`ORBISTOUN_WATCHPOINT`): a hardware breakpoint on an instruction fetch (`R/W=00, LEN=00`). It is **one-shot** -
the first hit snapshots all sixteen registers into statics (allocation-free, on the guest's stack) and then
the handler clears that slot's `Dr7` enable bit, so the instruction runs and the guest carries on. No
resume-flag or single-step machinery, and no infinite re-trap. The summary prints the snapshot: the register
state a function was entered with, i.e. its arguments (`rdi, rsi, rdx, rcx, r8, r9`). Provenance-clean - it
reads registers, never guest code.

**Verified.** Armed at `image+0xafcc08` (PPSA02664's null-write site), it fired once, printed the full
register state, disarmed, and the guest ran on - the summary reads
`execute breakpoint at ... hit 1 time(s); registers the first time: rax=0x0 ... rbx=0x4000 ... r14=0x20 ...`.
`fmt`/`clippy`/`cargo test` pass for the additions (44 tests); the pre-existing `enter` `too_many_lines` in
`lib.rs` is untouched debt, not from here.

**What the tool then corrected, which is the point of it.** The first guess from the snapshot was that
`r14 = 0x20` (below tlsf's `0x28` minimum) was the rejected pool size. Reading the instruction stream the
report captures just before `0xafcc08` killed that guess. The faulting instruction is `vmovdqu [r12], xmm0`,
and two instructions earlier `r12` is **explicitly zeroed** (`xor r12d, r12d`) - not a stale return value. That
`xor` is the target of a `je` taken when the preceding `call [rax+0x10]` - an indirect call through an object's
table - returns **zero**: on that branch the guest zeroes `r12` and writes through it. So `0xafcc08` is a
**virtual call returning null**, not the tlsf size path at all, and `r14 = 0x20` was a coincidence. (The tlsf
message still prints elsewhere in the run - whether the null method *is* the tlsf allocation wrapper is the
open question.) The next step the tool enables: an execute breakpoint at `0xafcc08 - a few bytes` to capture
`rax`, read `[rax+0x10]` for the method's address, then break there to see why it answers zero. This decision
is the tool; that it overturned its own first lead in one run rather than after an afternoon is the argument
for it.

Note the D450 non-determinism still applies: a run reaches `0xafcc08` or `0xb14be3` by a thread race, so the
breakpoint fires only on the runs that take its branch - which is why the snapshot says how many times it hit.
