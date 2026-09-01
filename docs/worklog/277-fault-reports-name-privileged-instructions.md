# 2026-09-01 - (/loop) Fault reports name privileged instructions + distinguish emulator bugs

The user asked for diagnostics so this class of fault is obvious next time rather than costing turns.
PPSA21564's `Cond.cpp:212` abort was hard to read for two reasons, both in the report, not the fault:

- An our-code fault still said `guest fault ... (inside <import>)`, where the import is the guest's last
  call (context), not the faulting function - so the header pointed at `__cxa_guard_acquire`/nearest-export
  when the real frame (`render_with`) was buried in the host stack. Cost a whole turn chasing the wrong fn.
- A privileged instruction raises a GP-fault the host reports as an access violation at 0xffffffffffffffff,
  which looks like a guest dereferencing -1 with no import to blame.

Changed `orbistoun-worker`'s reporter: (1) if the IP is outside the guest image, say loudly EMULATOR BUG and
that the faulting fn is in the host stack, not the header import; (2) decode the faulting instruction and, if
it is int/syscall/sysenter/hlt/ud2, name it and say the guest went under the library boundary (D378) - wants
a kernel handler, no import to blame; (3) explain 0xffffffffffffffff as a GP-fault, not a real -1 (D384).

Immediate payoff: PPSA21564's fault now reads as `int (a software interrupt)` = `int 0x41`, and the bytes
before it (`test eax,eax; je`) are the assert shape `if (rc==0) skip; trap`. So it is the engine's own
assert-abort, fired because some rc != 0 - not a printf bug, and not the _umtx_op syscall this investigation
earlier guessed (the guest records zero syscalls; that theory was wrong). Recorded D456. The upstream "what
makes rc != 0" is still open (no correlating HLE call; the 251 MB load-transformed eboot blocks file-RE).

On the user's bulk-syscall question: no new stubs. Numbers already map to named implementations
automatically, unknowns already answer ENOSYS loudly, and inventing unmeasured syscall behaviour is the
loud-stub anti-pattern; loudness (this change) is the right investment, and PPSA21564 issues no syscalls
anyway. report.rs additions are fmt/clippy/test clean; extracted two helpers to keep `emit` under the line
limit and cleaned two pre-existing const-placement lints in the same file. Pre-existing `enter` too_many_lines
(worker/lib.rs) left as separate debt.
