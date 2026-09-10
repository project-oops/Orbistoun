# 475. The report learned to name threads

**2026-09-09** - directed, continuing 474

474 left a mechanism sketched but not built: deliver a pending signal at the target's next thunk
dispatch. This pass measured whether it would fire before building it.

## It would not

```text
DIAG cross-thread raise at call 310985 of the run   (of 310986)
```

The raise is the **second-to-last call of the whole run**. The target makes no further call, so a
dispatch-entry check would never see the signal. A `println` ruled out a mechanism that would have
cost a load on every one of 310,986 dispatches and delivered nothing.

Two more rounds of hand instrumentation established the rest: the caller is an
`AssetGarbageCollectorHelper`, the target is `main`, and the call straight after the raise is
`sceKernelWaitSema`. A collector suspending a mutator and waiting for an acknowledgement.

## Every round re-derived what the process already held

The thread registry has names, handles and finished flags. The recorded-call ring has labels,
sequences and a host thread. **Nothing joined them**, so a report could say "fifteen threads exist"
or "the last forty-eight calls were one thread" and never that they were different threads.

`ThreadRecord` now carries `host`, filled in `become_thread` - on the thread itself, since a thread
is created by one and runs on another. `CallTrace` carries `threads`, and the report prints them
when the guest went quiet, grouped by name and sorted busiest-last (D651).

## What it then said, unprompted

```text
threads  15 guest thread(s), 13 with no call in the recorded window
         AssetGarbageCollectorHelper    x13              no call in the last 48 of the run
         main                           0x5e2d00000200   last called sceKernelSyncOnAddressWait at 310583
         (unnamed)                      0x5e2d00001dc0   last called sceKernelWaitSema at 310985
```

**`main` is parked inside `sceKernelSyncOnAddressWait`** - a wait orbistoun owns. Delivery to a
thread running native code is out of reach and always was; delivery to one asleep in this
project's own futex, at a point this project chose, is a bounded mechanism. Nobody asked for that
fact; it fell out of printing what the report should have printed all along.

## Surprises

- **The first version of the new report buried its own answer.** One line per thread, and thirteen
  identically-named Unity helpers pushed `main` - the thread the signal was aimed at - off the
  end. Grouping by name turned a fifteen-line block into three, and only then was it useful.
- **An empty thread name printed as blank columns**, which read as a continuation of the line
  above; the collector looked like part of `main`. Now `(unnamed)`.
- **The pass reversed itself twice.** Feasible (D650 sketch), then ruled out by the call index,
  then feasible again by a different route the report volunteered.

## Next

- Signal delivery to a thread parked in an orbistoun wait: mark pending, wake it out of
  `sceKernelSyncOnAddressWait`, run the handler, return it to the wait. Its honest limit is that a
  mutator spinning in guest code still never takes the signal.
- The `selfish-elf` differential, still claimed and still owed.
