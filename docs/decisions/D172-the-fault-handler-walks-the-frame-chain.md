# D172 - The fault handler walks the frame chain


**decided** · 2026-08-21

A fault address says *where* the guest died. It does not say who called it - and at the top
of a function, which is where a null dereference usually lands, the instruction pointer
alone is nearly content-free. `image+0x43c4` had survived six separate attempts partly
because there was no way to see what led to it.

The handler already had `rbp`. Walking the chain gives the guest's own call path:

```text
read of 0x0 while executing at image+0x43c4
  from image+0x4409
  from image+0x51c5
  from image+0x45ab
  from image+0x44c5
  from image+0x2f99
  from image+0xdc4f
  from image+0xde58
  from image+0xdbfb
  from image+0xaf        <- the entry point is 0x70
  from 0x7ff65a3732d2    <- host: enter_guest
```

**The entire path from the guest's first instruction to the fault**, and it says something
the fault address alone did not: this is startup, not game logic, and four of the frames
cluster within 0x1e0 bytes of each other - one small group of functions calling into itself.

### The care this needs, because of where it runs

It executes inside a fault handler on a thread that has already faulted once. A second
fault there replaces the report with silence, so:

- **Every read is bounds-checked against the stack region before it happens**, using the
  region table the reporter already maintains. Not after; a check after the read is not a
  check.
- The frame must be eight-byte aligned, and the chain must *climb* - anything else is a
  cycle, and a fault handler that loops dies with nothing said.
- Bounded at twelve frames. The chain is guest-controlled.

**An empty result is the ordinary case, not a failure.** A compiler may omit the frame
pointer and optimised code routinely does, so a walk that produces nothing means "not
available here" rather than "something went wrong". Reported as an empty list for exactly
that reason.

