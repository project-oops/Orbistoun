# D177 - `abort` must not return, and it was reporting the opposite of the truth


**decided** · 2026-08-21

With the previous generation parsing, two titles failed identically: `illegal instruction`
at `image+0x1595bc9`, 53 calls, four frames. **Identical across unrelated titles** is the
D152 signature - a shared path rather than title code.

The ordered call tail with call sites (D173) named it in one line:

```text
52  libc::abort   arg0=0xc10   from 0x400001595bc9
```

`abort` called **from the exact address that then trapped**. It is declared `noreturn`, so
a compiler emits an unreachable trap immediately after the call. `abort` was not declared
here at all, fell to the default stub, **returned**, and execution ran into that trap.

So the emulator was reporting `illegal instruction` at a meaningless address while the
guest was doing something perfectly clear: giving up. That is worse than an unimplemented
function - it is a report that contradicts what happened.

### The layering it needed

A guest that stops has decided to stop; how to stop is not the subsystem crate's business.
It does not know whether a trace is being written or where it goes. The worker knows, and
sits *above* the subsystem crates, so it cannot be called downwards.

`orbistoun-core::stop` is a handler the worker installs and the subsystems call - the
ordinary way to invert a dependency without breaking the spine (principle 6). The trace is
persisted before the process ends, the same as the fault and time-limit paths.

### A third outcome, which was being reported as the second

A run ends by faulting, by being stopped from outside on the time limit, or by the guest
deciding to stop. There was no field for the third, so a trace with no fault meant "ran to
the time limit" - and a guest that called `abort` was described as having run out of time.
Not merely imprecise: the opposite of what happened.

`CallTrace::stopped` records it, and the report now reads `fault  the guest called abort`.

