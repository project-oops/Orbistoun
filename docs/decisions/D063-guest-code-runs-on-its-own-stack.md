# D063 - Guest code runs on its own stack, entered through a switch

**decided** · 2026-08-19

Borrowing the host thread stack works right up until the guest overruns it, at which
point it corrupts the host frames below and the crash surfaces inside the emulator
rather than in the guest. A dedicated stack with a **guard page beneath it** turns that
into an immediate fault at an adjacent address, which says what happened. The guard is
reserved as part of the same span, so nothing can ever be placed in the gap.

**The register discipline is where this is subtle on Windows.** The guest may destroy
every System V caller-saved register; Windows host code expects `rsi`, `rdi` and
`xmm6`-`xmm15` to survive a call. Those sets disagree, so every one of them is declared
clobbered - omitting them corrupts host state on Windows only, silently, long after the
call returns. `r12` carries the host stack pointer across, because System V obliges the
guest to give it back.

**Entering is refused unless the entry point is inside an executable segment.** Jumping
to an address that is not executable is certain to fault, and the fault alone says
nothing; the refusal names the actual problem. This was not theoretical - it is exactly
what the worker test suite hit the moment entry was wired up, because an image with zero
relocations counts as fully linked.

**What is deliberately not built:** a real entry point expects a process stack image -
argument count, argument and environment pointers, an auxiliary vector - not a return
address. The entry is called as an ordinary function instead, which is honest about what
it is: enough to execute guest instructions and see how far they get, not enough to
satisfy a runtime that reads its own arguments.

