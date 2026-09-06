# 2026-09-03 - (/loop) A positioned read took the guest into the GPU

```
188 -> 197 distinct imports   fault MOVED: read of 0xa0 @0x1389269 -> read of 0x50 @0xf56e09
suites 125   tests 2012   clippy/fmt/identity clean
```

Twelfth cron tick. **The wall moved for the first time since D515.**

`sceKernelPread` was undeclared, while `descriptor::read_at` - the positioned read itself - had
been built all along and used by POSIX `pread`. One declaration and one wrapper.

## What it opened

```text
sceAgcCreateShader                  sceAgcDriverAddEqEvent
sceAgcDcbResetQueue                 sceAgcDriverSetHsOffchipParam
sceAgcDcbSetIndexSize               sceAgcDriverSetTFRing
sceAgcDcbWaitUntilSafeForRendering
sceKernelClockGettime   _sigprocmask   sceKernelWaitEqueue
```

**The graphics pipeline** - shader creation, command-buffer setup, driver ring configuration.
Reading its own asset files is what stood between this title and its renderer.

`sceKernelWaitEqueue` is worth noting separately: D524 built the queue's identity and
deliberately no delivery, because nothing waited. Something waits now.

## The wall five ticks orbited is behind us

The new fault's chain is `image+0xf269e7` - the return address of the call at `0xf269e2`, **the
memory-manager initialiser**. So `0xf23970` returns now, and the module no longer invokes the
scope hook before the memory manager exists.

Retrospective support for the reading D519 left open and D521 narrowed to one: the module *was*
on a path it would not take on hardware, and what put it there was upstream. **Support, not
proof** - nothing measured the console; what is measured is that answering two file calls made
the symptom go away.

## `ORBISTOUN_RETURN` is a return-value experiment, not an implementation switch

The first A/B forced pread to the placeholder and reported **197 either way**, which read as "the
answer does not matter". Wrong: **forcing a return does not stop the implementation running.**
The bytes still landed in the guest's buffer; only the count was replaced.

D525's A/B on `stat` was valid for the opposite reason - a guest branches on whether `stat`
succeeded, so replacing the return replaced the thing under test. The distinction is between a
call whose *answer* is the product and one whose *side effect* is.

The honest A/B is to unregister and rebuild, which is where the numbers above come from.

## The break is on the position, not the bytes

A seek/read/seek-back passes the byte assertion and fails the next one. Watched failing.

## And a correction: I read a capped list as a complete one

The report prints six findings and ends with `... and 23 more`. **I read the six as the whole set
and said "three stubs left" twice.** The trace has 28.

The tool was honest - it says how many it withheld, on the very next line. Check 4 turned on its
reader rather than on a filter: a *printed* list is a fact about the printing. Reading the trace
instead is what found `sceKernelPread` at all.

Decision: [D526](../decisions/D526-a-positioned-read-took-the-guest-into-the-gpu.md).
