# 2026-09-03 - (/loop) The event queue exists so a handle means something

```
suites 125   tests 2010   clippy/fmt/identity clean
wall unchanged: 181 distinct, read of 0xa0 at image+0x1389269
```

Tenth cron tick. `sceKernelCreateEqueue` and `sceKernelAddUserEventEdge` implemented; **delivery
deliberately not**, and that is the part worth reading.

## The arities came from the run, again

Neither was declared. In both, the third register holds `0x7fff_0001` - **orbistoun's own
placeholder, left by an earlier stub** - which is what fixes the arity at two. Same evidence that
fixed `CreateEqueue` in D516 and `scePthreadSetaffinity` in D523.

```text
CreateEqueue      arg0 writable, arg1 -> "eq to wait flip" / "flip equeu", arg2 = 0x7fff0001
AddUserEventEdge  arg0 a handle, arg1 = 0x1 then 0x2,                      arg2 = 0x7fff0001
```

The names are the guest's own. D522's register annotation is what made them readable in the
report at all.

## The out-parameter was the whole bug

Unimplemented, `CreateEqueue` **never wrote its out-parameter**, so the guest's handle kept
whatever it held and every later call was handed a value orbistoun never issued. Same shape as
D507 and D509, and invisible from the call that causes it. It now follows
`kernel_create_event_flag` exactly.

## The handle check, and how it was verified rather than assumed

`AddUserEventEdge` refuses a handle no queue answers, with the vendor `ESRCH` the event-flag
family answers (measured by obSCEne, not a placeholder - D125).

**A check like that is worse than no check if everything fails it**, and a run where every
registration was refused looks exactly like a run that made none. So the queues are reported:

```text
orbistoun: 2 event queue(s): "eq to wait flip" (0 event(s)), "flip equeu" (2 event(s))
```

The handle written by one call was kept by the guest and recognised by another - the round trip,
end to end. `"eq to wait flip"` holding nothing is the guest's own doing.

## Nothing is delivered, deliberately

**`sceKernelWaitEqueue` is called zero times.** It was the busiest import in the run until D516
stopped `sceVideoOutIsFlipPending` claiming a hundred and thirty-four million pending flips.

A queue with no reader does not need a queue in it, and the shape of a delivery path should be
decided by the first guest that actually waits - which will show what it expects to read back.

## Breaks watched to fail

```text
accept a handle nobody was given -> "promise delivery from a queue that does not exist"  FAILED
do not write the out-parameter   -> "a value orbistoun never issued"                     FAILED
```

## The project's own gate caught what I missed

`every_implemented_function_is_written_down` failed the workspace suite -
*"sceKernelAddUserEventEdge is implemented but nothing is recorded about it"*. I had recorded the
create and not the register. Nothing I did found that; the guard did, on its first run after the
change, which is what it is for.

**The wall did not move** - the equeue was never what blocked this title, it was a gap answering a
placeholder to a guest that could act on it.

Decision: [D524](../decisions/D524-the-event-queue-exists-because-a-handle-has-to-mean-something.md).
