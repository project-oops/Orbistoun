# D650 - The signal goes to another thread

**Status:** measured
**Date:** 2026-09-09

## The contract is now complete

`REQ-20260909T1445Z-2a90` asked obSCEne to repeat the exception-handler probe with a real thread
handle. Sweep 20260909-151910 answered every question D648 had to leave open:

| call | return |
|---|---|
| `raise(self, 30)` with a handler | `0x0`, **and the handler's flag reads 1 on the next line** |
| `raise(self, 31)` | `EINVAL`, with a valid handle - so 31 really is refused |
| `raise(1, 30)` | `ESRCH` |
| `raise(self, 30)` with no handler | **does not return** - the process takes the signal and dies |
| a worker's `raise(main, 30)` while main is in the handler | `0x0`, handler re-entered on main |

**Delivery is synchronous.** The flag settles it: the handler runs before the call returns. The
handler is passed the signal number in `rdi` - `0x1e`, exactly the value PPSA25872's own handler
opens by comparing against - and a context pointer in `rsi` and `rdx`.

Three things follow and are implemented: the same-thread delivering call, via the `call_guest`
path `execute_once` already uses; `EINVAL` for 31; and a new `StopReason::Signalled`, because a
raise with no handler ends the process and neither `Aborted` nor `Exited` could say so without
naming a call the guest never made.

## And it did not move PPSA25872 at all

The title raises on thread `0x5e2d00000200` while running on `0x5e2d00001dc0`. **The target is
not the caller.** A collector suspending a mutator - stop-the-world, which is what the whole shape
suggested from the beginning and what the trace never actually said until it was instrumented.

The measurement covers this case: the caller gets `0` and the handler runs *on the target thread*.
Orbistoun cannot do that. Nothing here can interrupt a thread already running guest code - guest
code executes natively, there is no interpreter loop to check a flag in, and the target is not
inside an orbistoun implementation at the moment the signal is raised.

## Why the branch refuses instead of answering zero

Answering `0` would move the wall today. It would also tell the collector that a thread had
acknowledged a suspension it never received - and the collector's next act is to read that
thread's registers and walk its stack, so it would proceed on state nobody produced. The failure
would surface far from here, as a corrupt heap or a hang in a different place, and nothing would
point back. That is the exact trade principle 3 exists to refuse, and it is worth naming that the
temptation was real: this is the furthest-reaching title in the corpus and one `0` would have
produced a visibly longer run.

## The concept this needs, named rather than built

Cross-thread delivery is a **mechanism**, not a stub, so it is recorded here rather than assumed
into existence. The shape it would take:

- a per-thread pending-signal slot, one atomic word, checked on the way into the thunk dispatch -
  which every guest call already passes through, so the cost is one relaxed load on a path that
  ran 310,986 times this run (principle 9);
- the handler run at that point, on the target thread, before the call it was about to make;
- and a caller that returns `0` **only once the target has taken it**, which is weaker than
  hardware - hardware delivers to a thread doing anything at all, this would deliver only to a
  thread that calls something next.

That last line is the honest limit and the reason this is a decision rather than a patch: a
mutator spinning in pure guest code would never take the signal, and a collector waiting on it
would hang exactly as it does now. It converts "never delivered" into "delivered at the next
call", which is a real improvement and not the contract.
