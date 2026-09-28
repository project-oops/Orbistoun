# D734 - A stateless leaf import links past the call trace

**Status:** decided
**Date:** 2026-09-28

An import with a leaf implementation has its stub jump straight to that implementation instead of
the shared trampoline. A leaf is a stateless function the guest calls so often that the traced path
is most of its cost; the first is `sceKernelGetProcessTimeCounter`. The subsystem crate declares
its leaves beside its implementations (`leaves()`), and each keeps its traced implementation too.

Only the stub's jump target changes. The guest slot still holds the stub's fixed address, so the
link plan (D724) is identical with or without leaves. The stub still loads its index; `leaf_entry`
counts the call in the same per-import counter the report reads, aligns the stack for the call
whatever the guest left it at (D159), and calls the implementation with the argument registers
untouched. A leaf spends no sequence number, writes nothing to the call ring, is not parked at
(D344) and does not count against the call budget (D238).

Leaves are linked once, after every diagnostic is installed. An import named by a dump, a forced
answer, a forced write or a policy write keeps the traced path, so a diagnostic works on a leaf
as on any import.

**Why:** a GL context times every phase of every draw. Bugdom's calls the process clock about
660,000 times a second, 99% of all its guest calls. On the traced path each call cost about 150 ns
under several guest threads, 13% of the guest thread, where the hardware's clock read costs tens of
nanoseconds. Measured on the same run three times each, the credits screen went from about 89 to
97 flips a second.

**Rejected:**
- Slimming the traced path: its cost under load is cache-line contention on the shared sequence
  number and ring. Dropping records would lose the trace; keeping them keeps the contention.
- Asking the SDK to read the clock less often: it is right on the hardware, where the call is cheap,
  and orbistoun would still pay the traced path for every title that reads a clock in a loop.
- Writing the host function's address into the guest slot: host addresses move with ASLR, and the
  link plan is keyed on fixed values.
