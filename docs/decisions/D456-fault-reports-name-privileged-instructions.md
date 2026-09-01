# D456 - Fault reports name privileged instructions and call an emulator bug an emulator bug


**measured** - 2026-09-01 (user-directed /loop; the user asked for diagnostics that make this class of fault obvious next time)

Chasing PPSA21564's `Conc/Cond.cpp:212` abort cost far more than it should have, and the reason was the fault
report, not the fault. Two report weaknesses sent the investigation down two wrong paths in turn:

- When the fault is in **orbistoun's own code**, the header still said `guest fault ... (inside <import>)`,
  where `<import>` is the guest's *last import call* - context, not the faulting function. Read literally it
  says "the guest faulted inside this import", and the real frame (`render_with`) was only in the host stack
  forty lines down. That is how a whole turn went to `__cxa_guard_acquire`/`sceLibcMspaceMalloc` (the nearest
  *exported* symbol) when the code that faulted was `render_with`.
- A **privileged or trap instruction** the guest executes - `syscall`, `sysenter`, `int`, `hlt`, `ud2` -
  raises a general-protection fault the host reports as an access violation at `0xffffffffffffffff`. That
  reads exactly like a guest dereferencing -1, and there is no import to blame, so it looks like a wild
  pointer of unknown origin (the same `0xffffffffffffffff` D384 already spent a dozen eliminations on).

**The change** (in `orbistoun-worker`'s fault reporter):

- If the instruction pointer is outside the guest image, the report now says, loudly, `EMULATOR BUG: the
  fault is in orbistoun's OWN code, not the guest's`, and states that the import in the header is context and
  the faulting function is in the host stack - not the "nearest implementation" line.
- The faulting instruction's first bytes are decoded, and if it is `int`/`syscall`/`sysenter`/`hlt`/`ud2` the
  report names it and says the guest has gone *under the library boundary* (D378) - it wants a kernel-level
  handler, not a library shim, and no import is to blame.
- An all-ones fault address is explained as a general-protection fault (misaligned SSE, or a privileged
  instruction), not a genuine read of -1 (D384).

**What this immediately made obvious.** PPSA21564's fault is the guest executing **`int 0x41`** - and the
bytes before it (`test eax,eax; je +...`) are the ordinary assert shape `if (rc == 0) skip; <trap>`. So the
crash is the engine's **own assert-abort trap**, fired because some `rc != 0`; `int 0x41` is how asobi aborts.
It is not a `printf` bug and not the `_umtx_op` syscall this investigation earlier guessed - the guest records
**zero** syscalls (it never reaches the syscall gadget), so the syscall theory was wrong. The report now says
this in three lines instead of costing three turns. The real open question is upstream and unchanged: *what
produces `rc != 0`*, which no traced HLE call correlates with and which the 251 MB load-transformed eboot
blocks reading from the file (D455).

**On bulking out the syscall set** (the user asked): no new stubs. The FreeBSD numbers are standard and
already harvested, and `orbistoun-service`'s `syscalls()` maps every number to its named implementation
automatically - so any syscall whose library twin is implemented is *already* handled, and unknown numbers
already answer `ENOSYS` loudly (D378). Bulk-implementing syscalls whose behaviour has not been measured would
be inventing it, which is the loud-stub anti-pattern. The right investment is loudness, which is this change;
and it would not have helped PPSA21564 regardless, which issues no syscalls.

Pre-existing debt noted, not introduced here: `orbistoun-worker`'s `enter` trips `too_many_lines` (113/100)
and the crate carries other pedantic drift; that is a separate cleanup. `report.rs`'s own additions are
`fmt`/`clippy`/test clean, and the reporter's tests pass.
