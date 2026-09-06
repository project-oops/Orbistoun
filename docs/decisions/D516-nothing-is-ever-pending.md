# D516 - Nothing is ever pending, and the guest left its frame loop

**guest-observed** - 2026-09-03 (three identical runs, before and after)

D515 got PPSA02664 to its present loop, where it span:

```text
1,294,359 calls (43.1%)  libSceVideoOut::sceVideoOutIsFlipPending
1,294,358 calls (43.1%)  libkernel::sceKernelWaitEqueue
```

`sceVideoOutIsFlipPending` was undeclared, so it landed on a stub answering the placeholder
`0x7fff_0001`. **It returns a count, not a verdict**, so a guest asking "are any flips
pending?" was told a hundred and thirty-four million of them were.

## The answer is zero, and it is this crate's own model rather than a convenient value

`orbistoun-video` already documents the model, above `video_out_submit_flip`:

> On hardware a flip is *queued*, and its completion count moves when a presenter picks it up
> at the next vertical blank. There is no presenter and no vblank here [...] so the honest
> model for a headless run is that a flip completes as soon as it is accepted.

A queue that empties on submit is a queue with nothing in it. So the count of flips queued and
not yet presented is zero at every moment a guest can observe it - the same sentence, read
from the other end. Nothing was invented; the value follows from a model that was already
written down and already drives `sceVideoOutGetFlipStatus`.

**And the guest had submitted nothing anyway.** The trace has one `sceVideoOutOpen`, one
`sceKernelCreateEqueue`, one `sceVideoOutAddFlipEvent` - and **zero** `sceVideoOutSubmitFlip`.
It was draining a queue it had never put anything into.

## What it bought

```text
before   161 distinct imports, 20,000,000 calls - spinning, never left the loop
after    182 distinct imports,    415,411 calls - left the loop, faults somewhere new
```

Three runs, byte-identical: 182 distinct, 415,411 calls, `read of 0xa0` at `image+0x1389269`.
**415,281 of 415,411 calls are answered by an implementation** - five functions and 130 calls
are all that still land on a stub.

Arity 1, the same shape as `sceVideoOutClose`. The guest passes `0x1`, the port
`sceVideoOutOpen` answered; the registers after it carry leftovers, two of them identical.

## The bad handle is refused, not told it is idle

Zero is a **count**, so answering it for a handle no port owns tells a guest that a port it
does not have is idle. That is D125's failure one value along, and it has its own test.

Both arms were broken and watched to fail - the count, and the refusal.

## What this cannot prove, said in the test

That hardware answers zero. It does not: a real port has a scanout, and a flip really is
pending until the next vertical blank. What is asserted is the model orbistoun can honestly
implement with no scanout at all, and the test says so rather than leaving a reader to assume
the stronger claim.

## The equeue is next, and the evidence is already in hand

`sceKernelCreateEqueue` is still unimplemented, and the run says exactly what it is for:

```text
arg0 = 0x7400012be2b8   (writable, in the guest's own mapping arena)
arg1 -> "eq to wait flip"        ... and, on the second call, "flip equeu"
arg2 = 0x7fff0001                 <- orbistoun's own placeholder, left by an earlier stub
```

So: **arity 2**, a handle written through `arg0`, and a caller-supplied name - which the guest
wrote itself and which names the queue's purpose. The third register holding orbistoun's own
placeholder is what establishes the arity is 2 and not 3.

Unimplemented, the out-parameter is never written, so the caller's handle keeps the zero it
held and every `sceKernelWaitEqueue` is passed a null queue. Recorded in the knowledge file
rather than acted on here, because implementing it means a real event queue **and** flip events
delivered into it, and that is a subsystem rather than a return value.
