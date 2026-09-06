# D560 - Events are delivered, from a published layout, and the wait does not block

**Status:** guest-observed
**Date:** 2026-09-04

## The question

`sceKernelWaitEqueue` was the largest unimplemented call in the corpus by two orders of magnitude
- **839 calls** in an honest run of PPSA02664, **12,924** once past the shader wall. D524 had
declined to implement delivery, and gave a good reason: *what a caller reads back out of a
delivered event is unknown*.

## Two stale facts found first

D524's reasoning rested on a number that had expired. Both `sync.rs` and `lib.rs` carried the
claim that the title *"never waits - `sceKernelWaitEqueue` is called zero times since the flip
count stopped lying (D516)"*, and declined the storage on those grounds. It was true when
written. The title now creates **four** queues, not two, and waits hundreds to thousands of times
per run. Both notes are corrected.

## The layout, and why it is not a guess

`sceKernelWaitEqueue` is `kevent(2)`, and the target kernel is FreeBSD-derived, so `struct kevent`
is the citable reference - the strongest oracle this project recognises (principle 1):

```text
0x00  u64  ident      0x0c  u32  fflags
0x08  i16  filter     0x10  i64  data
0x0a  u16  flags      0x18  u64  udata
```

**The size is the open question, and it is named rather than assumed away.** FreeBSD 12 appended
`uint64_t ext[4]`, taking the structure from 0x20 to 0x40. Every offset above is common to both,
so a guest reading only these six fields cannot distinguish them - the guard says so, because it
is a test the rival passes too.

`udata` is the one field that is not a hypothesis: `kevent` echoes the caller's own word back, so
it is taken from the **registration** rather than from the poster. The poster is a flip, which
knows its port and not what a waiter asked to have handed back.

## The wait does not block, deliberately

A NULL timeout means an indefinite wait on hardware, and every observed call passes NULL. Blocking
here would give the emulator a way to stop for ever with nothing able to wake it - in most paths
this title takes, the thread that would post the completion is the one that would be blocked - and
**a hang destroys a run's evidence where a busy loop merely costs time**.

So a queue with nothing ready reports zero delivered, which is what `kevent` reports on a timeout
anyway. That is honest and it is not free: it is why the call count is still four figures. A title
needing a real block needs a poster on another thread first.

## The half that was actually missing

Not the wait - the **post**. `sceVideoOutSubmitFlip` completed a flip and told nobody;
`sceVideoOutAddFlipEvent` was unimplemented, so the registration that would have given it a reader
was never recorded. The guest registered a flip event, submitted, and waited on a completion this
project had already performed and never announced.

Arity three, read off the guest's own call and corroborated at both ends: `arg1` is `0x1`, **the
port handle orbistoun's own `sceVideoOutOpen` answered**.

## What it changed

| | before | after |
|---|--:|--:|
| `sceKernelWaitEqueue`, honest run | 839 | **605** |
| `sceKernelWaitEqueue`, past the shader wall | 12,924 | **1,127** |
| queues with a flip event registered | 0 | 1 |

**Frames did not move, and that is not this change failing.** The guest still reaches one flip and
stops - at the shader wall in an honest run, and at the fabricated shader object's `+0x30` when a
region is planted past it. The frame loop is unblocked from the event side and still blocked from
the Agc side; a second frame needs both.

## What this does not establish

**That the structure is right.** The guest accepting what it is given and ceasing to re-ask is
consistency, not correctness - a guest proceeds happily past a field it never checks. `filter` in
particular is written as **zero because no lawful source here gives its value**, and a flip
completion certainly has one; a guest that branches on it is taking the wrong branch right now and
will say so by where it stops.

**Nor that `data` carries the flip argument.** That is an assumption - it is the only per-flip
value the caller supplied and `data` is the only field shaped to hold it - and it is the first
thing to change if the guest disagrees.
