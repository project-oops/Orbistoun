# D651 - The thread that was not calling

**Status:** measured
**Date:** 2026-09-09

## Three rounds of hand instrumentation to learn two facts

D650 established that PPSA25872 raises signal 30 on a thread other than the caller, and sketched a
delivery mechanism: a pending-signal slot checked on the way into the thunk dispatch. Before
building it, the question was whether it would ever fire - the target has to call *something* for
a dispatch-entry check to see the signal.

It would not. Measured:

```text
DIAG cross-thread raise at call 310985 of the run   (of 310986)
```

The raise is the second-to-last call in the entire run. The sketched mechanism is **ruled out by
measurement**, and finding that out cost a `println` rather than the mechanism plus its
hot-path load on all 310,986 dispatches.

Two more rounds established that the caller is an `AssetGarbageCollectorHelper` and the target is
`main`, and that the call *after* the raise is `sceKernelWaitSema` - a collector suspending a
mutator and then waiting for it to acknowledge. Textbook stop-the-world.

**Every one of those three rounds re-derived something the process already knew.** The thread
registry holds names, handles and finished flags. The recorded-call ring holds a label, a
sequence and a host thread. Neither could be read against the other, because nothing recorded
which host thread a guest handle was running on.

## The join, and what it costs

`ThreadRecord` gains a `host` field, filled in `become_thread` - on the thread itself, because a
thread is *created* by one thread and *runs* on another, so the host identity is only knowable
once it is the one asking. `orbistoun_thunk::host_thread` becomes public to supply it.

`CallTrace` gains `threads: Vec<ThreadNote>`, joined at collection time, and the run report prints
it **when the guest went quiet** - the case that had a fault site and a tail for every other kind
of ending and nothing at all for this one.

**Grouped by name, and that is the whole of its usefulness.** The first version printed one line
per thread, and Unity's thirteen identically-named helpers filled the block and pushed `main` off
the end of it - the thread the signal was aimed at, invisible in a report about the signal.
Grouped, and sorted so the busiest is last, the same run says it in three lines.

## What the three lines then said, which nobody had asked

```text
threads  15 guest thread(s), 13 with no call in the recorded window
         AssetGarbageCollectorHelper    x13              no call in the last 48 of the run
         main                           0x5e2d00000200   last called sceKernelSyncOnAddressWait at 310583
         (unnamed)                      0x5e2d00001dc0   last called sceKernelWaitSema at 310985
```

**`main` is parked inside `sceKernelSyncOnAddressWait`** - a wait orbistoun implements and owns.

That reverses this record's own opening. Delivery to a running native thread is out of reach; that
was never in question. But the target here is not running native code - it is asleep in orbistoun's
own futex wait, at a point orbistoun chose, holding a stack orbistoun can see. Marking a pending
signal on its handle, waking it out of that wait, running the handler there, and returning it to
the wait is a bounded mechanism at a place this project already controls.

Nobody asked for that fact. It fell out of printing what the report should have been printing all
along, which is the argument for the field rather than for the three `println`s that preceded it.

## The limit, stated before it is built

Delivery would reach a thread parked in an orbistoun wait and no other. A mutator spinning in pure
guest code still never takes the signal, and this title only works because Unity's mutator happens
to be blocked on a futex. That is a real restriction and it goes in the next record, not this one -
this one is about being able to see the difference.
