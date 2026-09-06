# D562 - The tail, worked in order of what a placeholder does

**Status:** guest-observed
**Date:** 2026-09-04

## The question

After the event and pthread work, eighteen non-Agc functions were called and unimplemented -
**twenty-two calls between them**, most of them once each. Ranked by call count there was nothing
there. Ranked by *what the placeholder does when the guest reads it*, two were actively dangerous
and the rest were not gaps at all.

## The order that mattered

`StubReturn::Unimplemented` answers `0x7fff_0001`. What that means to a guest depends entirely on
what the function was supposed to return:

| Call | What the placeholder becomes | Verdict |
|---|---|---|
| `setjmp` | **non-zero** - "you arrived here via longjmp" | actively wrong |
| `setlocale` | a `char *` the guest dereferences | D125, a wild pointer |
| `_sigprocmask` | the guest's own stack read as a signal mask | garbage in |
| `sceKernelUuidCreate` | an unwritten identifier | benign, block was zeroed |
| `clock` | a huge elapsed time | timing logic misled |
| the rest | a status the guest proceeds past | harmless |

`setjmp` is the sharp one and it is worth stating plainly: a non-zero return from `setjmp` tells a
program it got there by `longjmp` and sends it down an error-recovery path it was never on. **The
placeholder was worse than any answer**, and one call was enough to justify fixing it.

Implemented: `setjmp`, `setlocale`, `clock`, `_sigprocmask`, `sceKernelUuidCreate`,
`sceVideoOutConfigureOutput`.

## `setjmp` saves nothing, and that is the record

A real `setjmp` stores the callee-saved registers, the stack pointer and the return address. This
stores none of them, so a `longjmp` into the buffer would jump through uninitialised memory and
take the process with it.

**No title in the corpus imports `longjmp`** - that is what makes returning zero safe today, and
it is exactly the condition to check before it stops being. Implementing `longjmp` without first
implementing the save here is the dangerous change; this one is not.

## `clock`'s unit is the biggest assumption in the batch

Microseconds, on POSIX's rule that `CLOCKS_PER_SEC` is 1,000,000. **FreeBSD's own headers have
historically defined it as 128**, and the target kernel is FreeBSD-derived - so a guest compiled
against those reads a time roughly 7,800 times too large. Nothing here settles which the vendor's
SDK uses. First thing to change if a title's timing is wildly wrong.

## `sceKernelUuidCreate` is deterministic on purpose

A counter, not a random value. A UUID is meant to be unpredictable and this one is not, which is a
real trade taken deliberately: the only progress measure this project has is whether one run got
further than the last, and an identifier that changed every run would be a difference between two
traces that meant nothing - the same argument D256 makes for process time. Unique within a run,
which is what a guest using one as a key needs.

## What was deliberately not implemented, which is most of it

**Nine of the eighteen are decisions, not gaps**, and finding that out was the larger half of the
work:

- **`sceUserServiceGetLoginUserIdList`** - D346 already decided this. It writes a list whose
  layout is unmeasured, and the entry says so where it is declared. Implementing it would have
  reversed a recorded decision on no new evidence.
- **`sceCommonDialogInitialize`, `sceMouseInit`, `sceAppContentInitialize`** - their modules are
  declaration-only by design, and the deeper reason holds anyway: **reporting that a subsystem
  initialised when the subsystem does not exist is the exact stub principle 3 forbids.** A guest
  told the mouse is ready calls `sceMouseOpen` next and gets a placeholder.
- **`sceVideoOutSetBufferAttribute2`, `sceVideoOutGetOutputStatus`** - fillers, not setters. Both
  promise to write a structure whose layout no lawful source here gives. `ConfigureOutput` beside
  them *is* implemented precisely because it promises nothing about the caller's memory, which is
  the line between the two.
- **`scePthreadAttrGet`, `scePthreadAttrGetstackaddr`** - D561's ambiguity, unresolved.
- **`PS5Util::0xa96b2b178383025c`** - no name, so nothing to implement.

## What it changed

Nothing in the run, and that was expected: none of these were blocking, and the guest stopped at
the same shader wall with the same imports. **Twenty functions remain unimplemented, and every one
is either out for hardware (eleven Agc and Ampr) or a decision recorded above.**

## What this does not establish

**That any of the six is right.** `setlocale` answers the C locale because orbistoun implements no
other, not because the console does. `_sigprocmask` keeps a mask nothing consults. `clock`'s unit
is a coin toss between two citable conventions. Each is better than the placeholder it replaced,
which is a lower bar than correct and is the bar that was cleared.
