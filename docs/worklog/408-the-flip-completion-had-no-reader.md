# 408. The flip completion had no reader

**2026-09-04** - directed, while the Agc probe set is out for hardware

## What was done

Picked the largest unimplemented call in the corpus and implemented it. `sceKernelWaitEqueue` was
**839 calls** in an honest run of PPSA02664 and **12,924** past the shader wall - two orders of
magnitude above anything else, and every one of them a guest waiting for something nothing posted.

**The missing half was the post, not the wait.** `sceVideoOutSubmitFlip` completed a flip and told
nobody, because `sceVideoOutAddFlipEvent` was unimplemented and the registration that would have
given the completion a reader was never recorded. The guest registered, submitted, and then waited
on work this project had already done and never announced.

| | before | after |
|---|--:|--:|
| `sceKernelWaitEqueue`, honest run | 839 | **605** |
| `sceKernelWaitEqueue`, past the shader wall | 12,924 | **1,127** |

**Frames stayed at 1**, and that is the honest reading rather than a disappointment: the guest
still stops at the shader wall in an honest run, and at the fabricated shader object's `+0x30`
when a region is planted past it. The frame loop is unblocked from the event side and still
blocked from the Agc side.

## Where the layout came from

`sceKernelWaitEqueue` is `kevent(2)` and the kernel is FreeBSD-derived, so `struct kevent` is
citable - the strongest oracle here, and the reason this was worth doing while Agc is out for
hardware. D524 deferred delivery precisely because the layout was unknown; it was not unknown, it
was unlooked-for.

The size is still open - FreeBSD 12 appends `ext[4]` and doubles it to 0x40, leaving every offset
unchanged - and the guard says so, because it is a test the rival passes too.

## Two stale facts, and how they were found

D524's reasoning rested on a number that had expired: both `sync.rs` and `lib.rs` said the title
*"never waits - called zero times since the flip count stopped lying (D516)"*. True when written.
The title now creates **four** queues, not two, and waits hundreds to thousands of times a run.
The knowledge entry carried the same expiry - *"cannot deliver one, because what a caller reads
back is unknown"*.

All three corrected. This is the fourth stale-fact find of the day and the pattern is consistent:
they are always a number that was measured once and then reasoned from afterwards.

## Guards

Seven, each watched failing, a different parameter each time: `filter`/`flags` swapped; `data` and
`udata` swapped; posting ignoring registration and broadcasting; the poster's `udata` overriding
the registration's; `take` draining regardless of the request; `take` returning newest-first; and
every handle looking like a live queue.

## What is deliberately not claimed

The guest ceasing to re-ask is **consistency, not correctness**. `filter` is written as zero
because no lawful source here gives its value, and a real flip completion certainly has one - a
guest branching on it is taking the wrong branch right now and will say so by where it stops.
Putting the flip argument in `data` is an assumption too: it is the only per-flip value the caller
supplied and the only field shaped to hold it.

## Not blocked on the console

Nothing here waits on the Agc probe results, and nothing here overlaps them - this is libkernel
and VideoOut. The one place the two meet is the frame loop, which needs both.
