# D621 - The report paired a fault with another thread's calls

**Status:** measured
**Date:** 2026-09-08

## Four decisions built on a pairing nobody checked

D613, D615 and D620 all treat a thread blocked in `sceKernelWaitEqueue` as the wall in PPSA03416
and PPSA02664. The reason is that the run report prints them together:

```text
! the guest faulted at image+0x39f7c, read of 0xffffffffffffffff
    just before: libkernel::sceKernelWaitEqueue(0x5e2d0000ee40)
    just before: libkernel::scePthreadMutexUnlock(0x400001a2df80) -> 0x0
```

**The call tail is every thread's, and the fault is one thread's.** Nothing said which of those
lines happened on the thread that died, so they read as a sequence and were treated as one.

## They were different threads

The fault now names its own:

```text
! the guest faulted at image+0x39f7c, ..., on guest thread 0x5e2d00000a20
    just before: libSceAgc::sceAgcCreateShader(0x6000007fc538) -> 0x7fff0001  from image+0xf56dd1
    just before: libc::memcpy(0x74000edc0200)
    just before: libc::memcpy(0x74000edc0100)
    just before: libkernel::scePthreadSelf(...) -> 0x5e2d00000a20
    just before: libkernel::sceKernelWaitEqueue(0x5e2d0000ee40)  [another thread]
```

`scePthreadSelf` on the faulting thread answers `0x5e2d00000a20`, which is the handle the fault
reports - the join confirmed from two directions. The event queue was never on that thread.

**The actual wall is `sceAgcCreateShader` answering `0x7fff0001`**, orbistoun's `Unimplemented`
placeholder, which is still sitting in `rbx` at the fault three calls later. The guest builds a
shader descriptor with two `memcpy`s, asks for a shader, is handed a placeholder, and walks it.

That was in the evidence the whole time, one line below where anybody was looking.

## What changed

- `FaultSite` carries the guest thread handle and the host thread mark.
- `TracedCall` carries the host thread mark, written into the ring on the call path - one atomic
  store per call, allocation-free (principle 9).
- The fault finding lists **its own thread's** last four calls, then two of anybody else's,
  labelled `[another thread]`. The other threads are still shown: a guest faulting while another
  thread holds something is a real situation, and hiding it would replace one wrong reading with
  a blinder one.
- `inside_import_name` asks `current_call` rather than `last_call`, so the import a fault is
  attributed to is the faulting thread's. That is the same mistake D616 found in the mapping
  record, in the place it mattered most.

## The shape, again

D616 fixed `last_call` being used for "what am I inside". This is the same function used for
"what was *this* thread doing", and the fix is the same: two questions were being answered by one
function because, single-threaded, they have the same answer.

Both were invisible until a guest ran more than one thread and something blocked. Neither was a
wrong calculation - each was a *missing distinction*, which is the third time this session that
has been the finding.
