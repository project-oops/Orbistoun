# 410. The tail, and what was decided not to do

**2026-09-04** - directed, while the Agc probe set is out for hardware

## What was done

Eighteen non-Agc functions were called and unimplemented, twenty-two calls between them, almost
all once each. Ranked by call count there was nothing there. Ranked by **what the placeholder does
when the guest reads it**, two were dangerous:

- **`setjmp`** answered `0x7fff_0001`, which is **non-zero** - and a non-zero `setjmp` tells a
  program it arrived via `longjmp`, sending it down an error-recovery path it was never on.
- **`setlocale`** answers a `char *`, so the placeholder was a wild pointer the guest
  dereferences. The D125 shape, again.

Implemented six: `setjmp`, `setlocale`, `clock`, `_sigprocmask`, `sceKernelUuidCreate`,
`sceVideoOutConfigureOutput`. Down to **20 unimplemented, 22 stubbed calls** from 35 and 914 at
the start of the day.

## The larger half was deciding not to

**Nine of the eighteen turned out to be recorded decisions rather than gaps**, and reading the
record before writing code is what found that:

- `sceUserServiceGetLoginUserIdList` - **D346 already decided it**, in the declaration itself:
  it writes a list whose layout is unmeasured. Implementing would have reversed a decision on no
  new evidence. Fifth time today that reading the existing record changed what I did.
- `sceCommonDialogInitialize`, `sceMouseInit`, `sceAppContentInitialize` - reporting that a
  subsystem initialised when it does not exist is the exact stub principle 3 forbids. A guest told
  the mouse is ready calls `sceMouseOpen` next and gets a placeholder.
- `sceVideoOutSetBufferAttribute2`, `sceVideoOutGetOutputStatus` - fillers, not setters; both
  promise to write a structure no lawful source here lays out. `ConfigureOutput` **is** implemented
  because it promises nothing about the caller's memory, which is the line between them.
- `scePthreadAttrGet` / `AttrGetstackaddr` - D561's unexplained argument, still unexplained.
- `PS5Util::0xa96b2b178383025c` - no name, nothing to implement.

Everything still unimplemented is now either **out for hardware** (eleven Agc and Ampr) or one of
those.

## Surprises

**A gate caught me twice in one session.** `every_implemented_function_is_written_down` failed
after the pthread batch and again after this one - six knowledge entries missing each time. It is
the second-most useful thing in the tree after the guest itself, and both times it was faster than
noticing on my own.

**`setjmp` with no `longjmp`.** The binary imports `setjmp` and never imports `longjmp`, which is
what makes answering zero safe today and is written into the entry as the condition to re-check.
Implementing `longjmp` without first implementing the save would be the dangerous change.

## What is not claimed

Each of the six is **better than the placeholder it replaced**, which is a lower bar than correct.
`setlocale` answers the C locale because orbistoun implements no other, not because the console
does. `clock`'s unit is a coin toss between POSIX's 1,000,000 and FreeBSD's historical 128 - a
factor of 7,800, recorded as the first thing to change if a title's timing looks wrong.
