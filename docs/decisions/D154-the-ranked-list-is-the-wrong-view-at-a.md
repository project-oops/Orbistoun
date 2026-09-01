# D154 - The ranked list is the wrong view at a wall; the ordered tail is the right one


**decided** · 2026-08-20

A ranked list of imports answers "what does this guest spend its time on", which is the
right question for choosing what to implement next. At a wall it is the wrong question
entirely, and it cannot be made into the right one by making it longer.

`sceKernelDirectMemoryQuery` at 99.9% of calls tells you nothing about a guest that just
wrote through a null pointer. The only useful question there is **what did it call last**,
and a frequency ranking cannot answer that at any length.

The ordering was always recorded - the dispatcher has filled a ring of
`(sequence, index, arg0)` since it existed, sized at 8192 while no run has exceeded 400.
It simply never left the process. The trace persisted the aggregate and dropped the
sequence.

So `CallTrace` now carries the last 48 calls in order, and the run report prints them when
- and only when - there was a fault. Consecutive repeats are collapsed, because a guest
clearing memory calls `memset` three hundred times in a row and printing them individually
buries the two calls either side that matter.

**It paid for itself immediately.** The first run with it produced:

```text
sceKernelAllocateMainDirectMemory(0x1fe0000)
printf(...)
memset(0x0) x3
```

A guest asking for memory, being refused, printing an error, and clearing a buffer it
never got. That is `sceKernelAllocateMainDirectMemory` identified as a blocker, its
argument order established (the first argument is a length in both observed calls), and
the failure mode explained - from one view of data that had been collected all along.

`arg0` is kept alongside the label for the same reason, and earns it in the same trace:
`libSceVideoOut::0xb9b56b04b654a0ac(0x7fff0001)` is a guest passing `GuestError::Unimplemented`
back to us as a video-output handle. That is D125's failure caught live rather than
inferred from a fault address.

The general shape: **the data was already there.** What was missing was a view of it that
matched the question being asked. Worth remembering the next time something looks like it
needs new instrumentation.

