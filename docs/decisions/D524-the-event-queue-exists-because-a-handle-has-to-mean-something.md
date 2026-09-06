# D524 - The event queue exists so a handle means something, not so events can be delivered

**guest-observed** - 2026-09-03 (arities from a run's arguments; the round trip verified end to end)

`sceKernelCreateEqueue` and `sceKernelAddUserEventEdge` are implemented. What is *not*
implemented is delivery, and the reason is worth more than the code.

## The arities came from the run, again

Neither was declared. The report prints the arguments an unimplemented call received, and in
both cases the third register holds `0x7fff_0001` - **orbistoun's own placeholder, left there by
an earlier stub**. That is what fixes the arity at two, the same evidence that fixed
`sceKernelCreateEqueue` in D516 and `scePthreadSetaffinity` in D523.

```text
CreateEqueue         arg0 writable, arg1 -> "eq to wait flip" / "flip equeu", arg2 = 0x7fff0001
AddUserEventEdge     arg0 a handle, arg1 = 0x1 then 0x2,                      arg2 = 0x7fff0001
```

The names are the guest's own and say what the queue is for. D522's register annotation is what
made them readable in the report at all.

## The out-parameter is the whole bug

Unimplemented, `sceKernelCreateEqueue` **never wrote its out-parameter**, so the guest's handle
kept whatever it held - and every later call against that queue was handed a value orbistoun
never issued. The same shape as D507 (a query's third field) and D509 (a semaphore's eight
bytes), and invisible from the call that causes it.

It now follows `kernel_create_event_flag` exactly: refuse a null out-parameter, write the handle
through it, answer `OK`.

## Why the handle is checked, and how that check was verified

`sceKernelAddUserEventEdge` refuses a handle no queue answers, with the vendor `ESRCH` the
event-flag family already answers - measured by obSCEne, not a placeholder (D125).

**A check like that is worse than no check if everything fails it**, and a run where every
registration was refused looks exactly like a run that made none. So the queues are reported:

```text
orbistoun: 2 event queue(s): "eq to wait flip" (0 event(s)), "flip equeu" (2 event(s))
```

Two created, and the second holds both registrations - so the handle written by one call was
kept by the guest and recognised by another. The round trip is verified end to end rather than
assumed, and `"eq to wait flip"` holding nothing is the guest's own doing: it created that queue
and never registered against it.

## Nothing is delivered, deliberately

There is no storage for a pending event and no code to deliver one, because **`sceKernelWaitEqueue`
is called zero times**. It was the busiest import in the run until D516 stopped
`sceVideoOutIsFlipPending` claiming a hundred and thirty-four million pending flips; since then
the guest never waits.

A queue with no reader does not need a queue in it. Writing one would be an abstraction ahead of
its caller, and the shape of a delivery path should be decided by the first guest that actually
waits - which will show what it expects to read back.

## What this did not do

**It did not move the wall.** 181 distinct imports, `read of 0xa0` at `image+0x1389269`, three
runs. The equeue was never what blocked this title; it was a gap that answered a placeholder to a
guest that could act on it.

## And the project's own gate caught the omission

`every_implemented_function_is_written_down` failed the workspace suite:
`sceKernelAddUserEventEdge is implemented but nothing is recorded about it`. I had recorded the
create and not the register. Nothing here found that - the guard did, on its first run after the
change, which is what it is for.
