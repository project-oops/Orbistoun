# 2026-09-02 - (/loop) Blocking WaitEventFlag (kills a 304k spin) + atan2f/sincosf

Chased PPSA04263's post-TLS wall (`image+0x196b91a`, 333k calls in). The return column showed
`sceKernelWaitEventFlag` at 91% of calls - 304,583 - "the guest is repeating it rather than
progressing", and it was flagged unimplemented. Now that spawned threads run (D464), the guest uses
event flags to coordinate them, and the blocking wait was missing.

Implemented it (D465): `sync::event_flag_wait` blocks on the `Condvar` the flag already had, honouring
AND/OR and the clear bits, with a deadline-based timeout; `kernel_wait_event_flag` shims it. Verified:
332,914 -> **28,343 calls** (`-304,571`, the whole spin), the guest parking instead of busy-looping.

But it did **not** move the wall - the run still ends at the same `int 0x41` guest assert
(`image+0x196b91a`, `cd 41`), which sat behind the spin all along. The last calls before it are all
`rand`; the assert is the guest deciding to abort on some upstream value, the same hard class as
PPSA21564's `0x11ccd`. Not caused by the spin, and not (below) by the math either.

Also implemented, honest stub-reduction, since the run flagged them unimplemented and heavily used:
`atan2f` (180 calls; a one-line `binary_f32!`) and `sincosf` (90 calls; single-precision sine+cosine
written through two float pointers, the strtod shape - one float arg, two integer-register pointers).
Both standard (`known_by = published`), computed in `f32` to avoid a double rounding. Neither moved the
assert either. clippy/fmt/tests/knowledge-audit all clean across libc, kernel and hle.

So PPSA04263's remaining wall is a genuine guest assertion, reachable now only because three structural
fixes (map-into-reservation D460, MAPPING_BASE collision D463, per-thread TLS D464) took it from a
32-call death to here; WaitEventFlag then removed the spin masking it. Cracking the assert needs
tracing what the guest checks at `0x196b91a` back to the value we got wrong - deep work, and shared
with PPSA21564.

Next oracle-free: rather than keep hammering one title's assert, spread out - PPSA25872's `0x7b594e`
(39,929 calls in), a naming pass on the unnamed hashes (could unblock PPSA25872's `PS5Util::0xf948…`),
or the trivial `scePthreadAttrSet*` advisory setters. The two `int 0x41` asserts (PPSA04263, PPSA21564)
are the hardest remaining class and may want a person, or an obSCEne trace of what the assert reads.
