# D615 - A queue nobody registered, and two nobody waited on

**Status:** measured
**Date:** 2026-09-08

## What the report could not say

The run report has listed event queues since D524:

```text
4 event queue(s): "eq to wait flip" (1 event(s)), "flip equeu" (2 event(s)),
                  "UnityFTMFlipQueue" (0 event(s)), "EOP QUEUE" (0 event(s))
```

Registrations, and nothing else. Which means a queue nobody uses and a thread stuck waiting on
one read identically: a name and a count. Finding the second took an argument dump, a fault
register, and a hypothesis - and it is the difference between a guest that is idle and a guest
that is stuck.

The queue now carries two counters, and the line says what each was used for:

```text
4 event queue(s): "eq to wait flip" (1 registered, 0 waits, 0 delivered),
                  "flip equeu" (2 registered, 0 waits, 0 delivered),
                  "UnityFTMFlipQueue" (0 registered, 1 waits, 0 delivered)  <- waited on, never delivered,
                  "EOP QUEUE" (0 registered, 0 waits, 0 delivered)
```

The starvation is called out rather than left to arithmetic. A reader scanning a report should
not have to notice that two numbers differ.

## What it says, in one line, that took an afternoon to establish

**The guest waits on the one queue with no registrations, and never waits on either queue that
has them.**

- `sceVideoOutAddFlipEvent` registers flip completions on `eq to wait flip` and `flip equeu`.
  Both work. Both are waited on **zero** times.
- The render thread blocks on `UnityFTMFlipQueue`, which has **nothing** registered against it,
  because the call that would register it - `sceAgcDriverAddEqEvent` - is unimplemented.

So the earlier reading was right and imprecise. It is not that AGC completions are missing in
general; it is that the queue the guest actually waits on has no producer *at all*, while the two
queues orbistoun does feed are ones this guest never asks about.

That sharpens what a fix has to do. Registering `sceAgcDriverAddEqEvent` against
`UnityFTMFlipQueue` is necessary and **not sufficient**: `post_event` matches a registration by
identifier, `sceVideoOutSubmitFlip` posts under the *port handle*, and the AGC call does not
appear to carry one - its second and third arguments are both zero. A registration that no post
can match is the same wait, reached more slowly, which is what D613 predicted before this line
existed to show it.

## The same in both titles

PPSA02664 and PPSA03416 produce **the identical four queues, the identical traffic, and the
identical fault** at `image+0x39f7c`. Two different titles, one Unity engine, one wall. Whatever
fixes it fixes both, and a measurement taken against either is a measurement about both.

## Counted before the wait, not after

`waited` increments before blocking. A wait that never returns is still a wait that happened, and
counting on the way out would have made the one case this exists to show - a thread stuck for the
whole run - contribute nothing.

## And three of its six arguments are not arguments

The handle in the report line joins it to the argument dumps, which is what it is for. The dump
of the two `sceAgcDriverAddEqEvent` calls reads:

```text
arg0 = 0x5e2d0000ee40      arg0 = 0x5e2d0000efe0
arg1 = 0x0                 arg1 = 0x0
arg2 = 0x0                 arg2 = 0x0
arg3 = 0x7ff7e75fcab0      arg3 = 0x7ff7e75fcab0
arg4 = 0x28635e15040       arg4 = 0x28635e15040
arg5 = 0x17                arg5 = 0x20
```

`0x17` and `0x20` on two calls that register two different kinds of queue reads exactly like an
event type, and I was one sentence from writing that down. **Two runs of one build say it is
not**: `arg5` reads `0x80` and `0x60` in the next run, and `arg3` and `arg4` are host addresses
that change every time. Only `arg0` is stable, and the two zeroes with it.

So the call takes at most three arguments and the declaration's six is orbistoun's own guess.
The surplus registers hold whatever the caller last put there, and the dump has no way to know -
it prints six because six is what it captures.

**The tell was available in one run and did not read as one.** `0x7ff7e75fcab0` is a *host*
address; every address family orbistoun hands a guest is in `docs/ADDRESS_MAP.md` and none of
them looks like that. The report already says "in no span this run published as readable", which
is true and reads as *the guest passed a pointer we cannot resolve* rather than *this register
was never set*. The distinction is worth making and is not made yet.

The general rule this session keeps arriving at from different directions: **a value that is
available is not a value that was established**, and the two runs of one build that D181 asks for
are what separates them.

## The tell is a sentence now

`describe_unreadable` compares an address-shaped value against the **envelope** of everything this
run published - lowest region base to highest end - and says when it falls outside:

```text
arg3 = 0x7ff68140cab0 -> in no span this run published as readable, and address-shaped
                       - and outside every region this run gave the guest
                         (0x400000000000..0x74000d080000)
```

Two findings that read identically before: *a pointer into something this run never declared* and
*a register the call never set*. The first is inside the envelope, the second is not.

**Outside the envelope is not proof of a stale register**, and the sentence does not say it is. A
guest can compute a wild pointer, and `orbistoun-abi` still hands one out in two places. It is a
fact about where the value sits; the reading is the reader's.

The test asserts both halves, because the useful sentence is the one that does **not** always
fire: a description that appended "outside every region" unconditionally would be true of nothing
in particular and would read as though it had checked. Made to fail by forcing the condition true,
and the second half caught it.
