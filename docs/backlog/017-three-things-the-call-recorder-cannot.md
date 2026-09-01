# Three things the call recorder cannot do yet


`orbistoun-thunk`'s dispatch path records every guest call into a fixed-size atomic ring,
drained by `orbistoun-worker` into the `CallTrace` a run reports from. It is lock-free and
allocation-free, which it has to be: one title makes ninety-nine million calls through it in
twenty seconds.

Three gaps, none urgent, all cheap where the recording already is:

- **No thread id.** Every call is attributed to the run rather than to a thread. Costs
  nothing today because no guest has spawned a second thread, and starts mattering the
  moment one does - an interleaved trace with no thread column cannot be read at all.
- **Only the first argument is kept in order.** `arg0` is in the ring; the rest arrive as
  dumps, which are capped and attached per-import rather than per-call.
- **No return value**, so "what did we answer" is inferred from the stub policy rather than
  observed.

And one that is not a field but a shape: **the ring holds 8192 calls and then keeps only
counts.** Bounded on purpose - an unbounded log would allocate on the call path - but it
means a long run's ordering is lost after the beginning. Writing the ring out periodically
would fix that without putting anything new on the call path, because a drain is not a
per-call concern.

A crate existed declaring all of this as types (`CallEvent`, `Sink`, `CountingSink`) and
implementing none of it. It was deleted rather than revived: the `Sink` trait is the one part
that cannot survive the workload - a dynamically dispatched call per guest call is exactly
the "sink that blocks a guest thread" principle 9 forbids - and the fields are cheaper to add
to the ring than to route through a trait object (D211).

## The three current walls

Written down because they are what "next" means for the emulator itself, and a wall
described only in a conversation is one somebody rediscovers from scratch. All three are
**phase 4 completion** - getting one guest through startup - not phase 5, despite phase 5
being where the roadmap says we are (2026-08-24).

