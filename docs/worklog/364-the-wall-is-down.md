# 2026-09-03 - (/loop) The wall is down: the guest reached its frame loop

```
before   44 distinct imports,      2,077 calls, fault: read of 0x8 at modules+0x13dca44
after   161 distinct imports, 20,000,000 calls, no fault - reached the call budget
suites 123   tests 2003   clippy/fmt/identity clean
```

**The first scheduled wakeup that actually fired** - a 20-minute cron, after twenty by hand.

## D514 named it; this ran it

`sceKernelLoadStartModule` did the `Load` half and never the `Start` half. Now, for a `/app0`
path, it looks the module up and runs its `DT_INIT` and `DT_INIT_ARRAY` in ABI order.

## Both halves D514 called substantial were already built

- **Entering guest code from a shim** is `thread::call_guest(entry, args)` - a synchronous
  call on a fresh reentrant stack, **in the same crate**, built for `call_once` initialisers
  and `atexit` handlers. Nothing new was needed.
- **Reaching the module table from the kernel** has an exact precedent one screen away:
  `note_region`, which the loader already calls for every placed module (D446).
  `note_module_initialisers` sits beside it.

The estimate was written from the shape of the problem rather than from the tree - D510's
lesson, again, in the direction that costs work rather than saves it.

## The comparison was wrong first, and the fix was to report more

The guest asks for `Il2CppUserAssemblies.prx`; the loader knows it by the **import library
name**, `Il2CppUserAssemblies`, no extension. Nothing matched and the run said *"the loader
recorded no initialisers for them"* - which reads as **never placed**, a different problem.

Two wrong inferences followed before it was measured:

- *"`imports` shows no `PS5Util`, so it is not placed."* False - `title_modules_for` derives
  what to place from the **library table**, not the import list. A negative from a filter is a
  fact about the filter (check 4).
- The two paths were first identified by arithmetic on the `snprintf` lengths (31 and 44
  characters). Right, and not evidence.

The fix for both was not a better inference. **The report now names what the loader placed:**

```text
loader placed with initialisers: Il2CppUserAssemblies(1 init), PS5Util(1 init), libc(1 init)
```

Breaking the stem match now prints the placed list and the not-started list side by side, so
the bug reads as what it is.

## Where the guest is now

```text
9,794,399 calls (48.9%)  libSceVideoOut::sceVideoOutIsFlipPending
9,794,398 calls (48.9%)  libkernel::sceKernelWaitEqueue
```

411,084 calls of real work, then a **present loop** - asking whether a flip has finished and
waiting on an event queue, neither of which orbistoun answers. A different kind of wall: not a
missing mechanism but an unanswered question, in the two subsystems principle 6 reaches last.

Deterministic at the new position: six interleaved runs at a 400,000-call budget, 138 distinct
every time, both heap directions. The up/down branch difference is gone - it was about an
early allocator decision the guest is now far past.

## Broken and watched to fail

```text
stem match compares the whole file name  ->  "no initialisers recorded" for a module that has
                                             them, beside "loader placed ... ARecordedModule"
```

## What this does not establish

That the constructors did the right thing - they ran and the guest proceeded, a 1-bit oracle
answering yes. And **not** that every module should be started: `libc` is placed with an
initialiser and does not run one, because the guest never asks. Only an explicit
`sceKernelLoadStartModule` starts anything; whether the platform initialises import-bound
modules at load time is unmeasured, and starting everything to make something work would be
inventing behaviour.

Decision: [D515](../decisions/D515-starting-a-module-took-the-wall-down.md).
