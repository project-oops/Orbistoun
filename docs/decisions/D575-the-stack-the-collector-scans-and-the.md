# D575 - The stack the collector scans, and the wall three titles shared

**Status:** measured
**Date:** 2026-09-07

## One signature, three titles

After the futex wait was modelled (D573), PPSA25872 died in its own modules reading
`0x600000802068`. PPSA02664 and PPSA03416, re-run the same afternoon, died at `0x6000008023e0` -
the same page. The main stack's usable span ends at `0x600000801000`, D445 keeps one readable
page above it for a runtime that over-reads its argument block, and all three faults sit in the
**first unmapped page above that**. A scan walking upward from the stack pointer, past a bottom
it believed was higher than the real top.

The three share a module, `PS5Util`, and share two calls, made once each and answered with the
placeholder: `scePthreadAttrGet` and `scePthreadAttrGetstackaddr`. Their captured arguments,
read together with the tail:

```text
scePthreadSelf()                         -> 0x1929d12f240
scePthreadAttrGet(0x1929d12f240, attr)                          placeholder
scePthreadAttrGetstackaddr(attr, &address)                      placeholder
scePthreadAttrGetstacksize(attr, &size)                         0
```

That is the sequence a garbage collector runs to find the bottom of the stack it must scan:
FreeBSD's `pthread_attr_get_np` on itself, then the stack address and size, then
`bottom = address + size`. With the address never written, the bottom was arithmetic on
whatever the stack slot held, and the scan ran off the top.

## The model

`scePthreadAttrGet(thread, attr)` fills an initialised attribute object from the thread's record:
stack address and size, priority, policy, affinity, guard size. The stack comes from the thread's
own record - `ThreadRecord` now carries the span a spawned thread reserved - and for the thread
the guest was entered on, which reserved nothing, from the span the worker told this crate about
(D275). **A thread with neither is refused**, not described: an attribute object holding a
made-up stack is exactly the wrong answer this exists to stop, and the test watches it refuse.

`scePthreadAttrGetstackaddr(attr, out)` reads the address back, pointer-wide like the size beside
it. A fresh object answers zero and success, which is FreeBSD's own behaviour for an unset
attribute and a real value rather than a placeholder.

## Measured

| Title | limit | before | after | verdict |
|---|--:|---|---|---|
| PPSA25872 | 12 s | 121 imports, crash at modules+0x117e1b | **141**, call budget | FURTHER |
| PPSA02664 | 25 s | 119, crash at modules+0x147f8b8 | **197**, crash at image+0x39f7c | FURTHER |
| PPSA03416 | 12 s | 119, crash at modules+0x1610b38 | **186**, a breakpoint | FURTHER |

**This closes the regression worklog 421 left open.** PPSA02664 is back at its record's 197;
PPSA03416 at 186 against 187; PPSA25872 is past its record's 129 and holds a new one. The
records were never ahead of the code - the code had been reaching the collector's first scan
only sometimes. With thirteen threads spinning on the futex, whether the main thread reached its
first collection inside the limit was a matter of scheduling; with the wait modelled it reaches
it every time. That reading is consistent with every run and is not proved by any of them: the
traces from 2026-09-04 are gone.

## What each title hits next

- **PPSA25872** spins 19.7 million times on `PS5Util::0xf948d02a4f9f5ace`, and `PS5Util.prx`
  **exports that hash** (`+0x2a0`), as it does the other one the executable imports from it. Both
  land on stubs. D484 binds a title's own exports ahead of the stub table; this pair is not
  bound, and the reason is in the linking step, which this session did not open.
- **PPSA02664** polls `sceKernelWaitEqueue` against `sceKernelGetProcessTimeCounter` after
  `sceAgcDriverAddEqEvent` answered the placeholder twice, then reads `-1`. A GPU event that was
  never registered, waited for: the Agc side, not this one.
- **PPSA03416** asks `sceKernelMprotect` for 256 MiB of write access starting inside its own
  module. The range is module memory rather than a mapping this crate owns, so it is refused
  with `EINVAL` as the console refuses an invalid mapping.

  **The rest of this entry's reading of that title is withdrawn (D576).** It said the guest
  then entered the stub table off a thunk's start, which came from a fault message that named
  stub padding for every breakpoint without looking at the address. The address is in the
  title's own module code and the stub table does not cover it.

  **The refusal is the wall, and D576 measures it.** The guest checks this return, takes its
  failure path, and calls a routine that was not supposed to come back. Answered with success
  the title reaches 192 imports and `image+0x1389269` - the wall its record already held.

## What this does not establish

**That the address is the stack's lowest byte.** FreeBSD's is, three collectors proceeding past
their wall is consistent with it, and a collector whose bottom was merely *higher than sp* would
proceed just as well past the first scan. A probe reading the two values on hardware settles it.

**The detach state and scope** of a running thread are left as the object holds them.

**Nor the errno for an unknown thread**, which answers the project's placeholder.
