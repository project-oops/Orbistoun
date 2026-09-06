# D526 - A positioned read took the guest into the GPU, and the wall finally moved

**guest-observed** - 2026-09-03 (A/B by unregistering, three samples)

`sceKernelPread` was undeclared. `descriptor::read_at` - the positioned read itself - has been
built the whole time, used by POSIX `pread`. One declaration and one wrapper later:

```text
without   188 distinct, read of 0xa0 at image+0x1389269
with      197 distinct, read of 0x50 at image+0xf56e09
```

**The fault moved.** It had been at the same instruction since D515.

## What it opened

```text
sceAgcCreateShader                    sceAgcDriverAddEqEvent
sceAgcDcbResetQueue                   sceAgcDriverSetHsOffchipParam
sceAgcDcbSetIndexSize                 sceAgcDriverSetTFRing
sceAgcDcbWaitUntilSafeForRendering
sceKernelClockGettime   _sigprocmask   sceKernelWaitEqueue
```

**The graphics pipeline.** Shader creation, command-buffer setup, driver ring configuration.
Reading its own asset files is what stood between this title and its renderer.

`sceKernelWaitEqueue` appearing is worth noting on its own: D524 built the queue's identity and
deliberately built no delivery, because nothing waited. Something waits now.

## The wall the last five ticks orbited is behind us

The new fault's call chain is `image+0xf269e7` - the return address of the call at `0xf269e2`,
which is **the memory-manager initialiser**. So `0xf23970` returns now; the module no longer
invokes the scope hook before the memory manager exists.

That is retrospective support for the reading D519 left open and D521 narrowed to one: the module
*was* on a path it would not take on hardware, and what put it there was upstream - a file call
that did not answer. It is support, not proof. Nothing measured the console; what is measured is
that answering two file calls made the symptom go away.

## `ORBISTOUN_RETURN` is a return-value experiment, not an implementation switch

The first A/B here forced `sceKernelPread` to the placeholder and reported **197 either way** -
which read as "the answer does not matter". It is wrong, and the reason is worth writing down:

**forcing a return does not stop the implementation running.** The bytes still landed in the
guest's buffer; only the count was replaced. For a function whose effect *is* a side effect,
this simulates nothing.

D525's A/B on `sceKernelStat` was valid for the opposite reason - a guest branches on whether
`stat` succeeded, so replacing the return replaced the thing being tested. The distinction is
between a call whose *answer* is the product and a call whose *side effect* is.

The honest A/B is to unregister the implementation and rebuild, which is what the numbers above
come from.

## What is shared and what is not

The positioned read is `descriptor::read_at`, shared with POSIX `pread` - it reads at an offset
**without moving the descriptor's position**, which is the whole point and is why it is not a
seek, a read and a seek back. Failure differs, as it does for `stat` (D525): the vendor `EBADF`
sign-extended, which obSCEne measured for `read`, `lseek` and `write` against a bad descriptor
(D439), never `-1`.

The test breaks on the position, not the bytes: a seek/read/seek-back passes the byte assertion
and fails the next one. Watched failing.

## And a correction: I read a capped list as a complete one

The run report prints six findings and ends with `... and 23 more`. **I read the six as the
whole set and said "three stubs left" twice.** The trace has 28 unimplemented functions.

The tool was honest - it says how many it withheld, on the line after. This is check 4 turned on
its reader rather than on a filter: a *printed* list is a fact about the printing. The real work
list is the trace, and reading it is what found `sceKernelPread` at all.
