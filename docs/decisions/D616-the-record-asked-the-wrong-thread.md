# D616 - The record asked the wrong thread

**Status:** measured
**Date:** 2026-09-08

## What the mapping list said

`ORBISTOUN_TRACE_MAPS` names, for every mapping the guest was given, the call it was made during:

```text
26  call 459349  0x740001980000 +0x80000  r arena  during libc::memcpy
31  call 459433  0x740001b00000 +0x40000  r asked-for  during libkernel::scePthreadAttrInit
33  call 459525  0x740001b80000 +0x40000  r asked-for  during libc::memcpy
35  call 459591  0x740001c40000 +0x40000  r asked-for  during libkernel::scePthreadCreate
```

**`memcpy` maps nothing.** Nor does `scePthreadAttrInit`. Those mappings were made by
`sceKernelReserveVirtualRange` and `sceKernelMapDirectMemory` on one guest thread and labelled
with whatever a *different* thread had most recently entered.

## Why

`orbistoun_thunk::last_call` reads the whole recording ring and answers with the newest call by
sequence - across every thread:

```rust
let newest = (0..MAX_RECORDED_CALLS)
    .filter(|i| RING[*i].load(Ordering::Relaxed) != 0)
    .max_by_key(|i| RING_SEQ[*i].load(Ordering::Relaxed))?;
```

That is the right answer to *"what was this process doing"*, which is the fault handler's
question and the reason the function exists. It is the wrong answer to *"what am I inside"*,
which is what the mapping record asks - and while the guest was single-threaded the two agreed,
so the difference had no way to show.

The list is truthful about *where* and *how much*; it was wrong about *who*, in the field a
reader uses to decide which implementation to look at.

## The change

`current_call` is a thread-local `u32` - the import this thread is inside, plus one, zero for
none - set around the handler and restored after. A stack rather than a store, because calls nest
through callbacks, and restoring the outer value keeps its identity for the rest of its body.

One word in thread-local storage, written twice per call. Recording must not allocate and must
not change what it observes (principle 9); this does neither.

`last_call` is unchanged and keeps its own meaning, which is now written down beside it. Two
questions that were being answered by one function are two functions.

```text
26  call 459728  0x740001e00000 +0x80000  r arena      during libkernel::sceKernelReserveVirtualRange
32  call 459978  0x740002080000 +0x400000  r asked-for  during libkernel::sceKernelMapDirectMemory
```

## And the queue item it retires

"The mapping sequence still varies where the import count no longer does" has been on the work
list since D604. It is not a defect to fix.

Two runs of one build agree on the first twenty-six mappings exactly and diverge from the point
the guest's second thread starts allocating. The count differs too - 61 mappings in one run, 57
in the next - because each thread gets as far as a wall-clock budget lets it. That is a
multithreaded guest under a time limit, and removing it needs a deterministic scheduler, which is
a concept nobody has proposed.

What matters is already stable: the distinct import count, which is what every progress
comparison rests on, has been 193 four times and 196-197 four times since D604 fixed it. The
mapping order was never the measurement.
