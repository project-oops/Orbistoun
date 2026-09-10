# 474. The target is not the caller

**2026-09-09** - directed, continuing 473

`REQ-20260909T1445Z-2a90` came back with the branch the previous sweep could not reach - a raise
with the leg's own `scePthreadSelf()` handle - and settled the whole contract in one go.

## What was implemented

**Delivery is synchronous.** The probe's handler sets a flag; the flag reads 1 on the line after
`raise` returns. So the handler is called here and now, nested on the calling thread, and
orbistoun does it through the same `call_guest` path `execute_once` has used for `call_once`
initialisers. The handler gets the signal number in `rdi` and null for the context pointer - the
structure's layout is not published, and a handler that walked an invented one would read
fabricated fields rather than fault at a named address.

Also in: `EINVAL` for signal 31, now that it was measured with a *valid* handle and the D648
ambiguity is gone; and `StopReason::Signalled`, because a raise with no handler does not return -
the process takes the signal and dies, and neither `Aborted` nor `Exited` could say that without
naming a call the guest never made.

## And PPSA25872 did not move

```text
DIAG raise thread=0x5e2d00000200 signum=0x1e current=0x5e2d00001dc0
```

**The target is not the caller.** The title raises on one thread while running on another - a
collector suspending a mutator. Hardware delivers that; the caller gets `0` and the handler runs
on the *target*. Orbistoun cannot: guest code runs natively, there is no loop to check a flag in,
and the target is not inside an orbistoun implementation when the signal is raised.

The branch refuses. Answering `0` would move the wall today and tell the collector that a thread
had acknowledged a suspension it never received - after which it reads that thread's registers and
walks its stack. The failure would surface somewhere else entirely, as a hang or a corrupt heap,
with nothing pointing back here (D650).

## Surprises

- **The trace never said "cross-thread"; one `eprintln` did.** Every report from this title showed
  the raise, its arguments, and the handle - and the handle being *a different thread's* was
  invisible because nothing printed the current one beside it. Two numbers, and the whole shape of
  the wall changed.
- **The wall got harder to move and much better understood.** Before this pass it was "we do not
  know what raise returns". Now every return is known, the delivering call is implemented, and
  what remains is one specific mechanism with a named cost.

## Next

- Cross-thread delivery: a per-thread pending-signal slot checked on the way into the thunk
  dispatch. Sketched in D650, deliberately not built in the same pass that discovered the need.
  Its honest limit is that a mutator spinning in pure guest code never takes the signal.
- The `selfish-elf` differential, still claimed and still owed.
