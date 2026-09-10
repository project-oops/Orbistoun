# D613 - A wait that did not wait, and a record that answered for somebody else

**Status:** measured
**Date:** 2026-09-08

## What the shared wall pointed at

`image+0x39f7c` is where both PPSA02664 and PPSA03416 now stop (D612). The instruction is a
structure walk - `mov rsi, [rsi+0x30]` then `mov edx, [rsi + rdx*4]` - and at the fault `rsi`
holds `0x542e2e00776f6c66`, whose bytes are `flow\0..T`. A pointer field holding string data.

The run report's own evidence names what happened immediately before:

```text
just before: libkernel::sceKernelWaitEqueue(0x5e2d0000ee40) -> 0x0
just before: libkernel::sceKernelGetProcessTimeCounter(0xf4) -> 0x2349bfe
```

And dumping that call's arguments:

```text
arg0 = 0x5e2d0000ee40        the queue
arg1 = 0x610084800f80        the event array - all zeroes
arg2 = 0x1                   one event wanted
arg3 = 0x610084800fa4        where the count goes
arg4 = 0x0                   no timeout: wait indefinitely
```

**The guest asked to wait indefinitely for one event and was told, immediately, that it had
succeeded.** The array it passed was never written. `sceKernelWaitEqueue` collected whatever
happened to be pending - nothing - and returned `OK` regardless.

That is `kevent(2)`, which blocks until at least one event is ready or the timeout elapses. It is
also the D171 shape exactly: an out-parameter left as the caller set it, under a success code.

The cost, in one run: **3,853 waits against 44 flips**, from two call sites. The guest loops
until an event arrives, never blocks, and reads an all-zero event structure every time round.

## The fix

`GuestEqueue` gets a condition variable, `post_event` signals it, and `wait_events` blocks while
the queue is empty - `Blocking::Forever` for a null timeout pointer, `Blocking::Until` for a real
one. Nothing arriving in time answers `ETIMEDOUT` rather than success, and the constant is now
shared with the event-flag wait instead of being declared twice for one condition.

`take_events` is `wait_events(…, Blocking::Never)`, so the non-blocking readers are unchanged and
there is one implementation rather than two.

**It did not move the wall**, and that is worth saying plainly: 197 distinct imports either way,
the same fault at `image+0x39f7c`. What it fixed is a wrong answer, which is its own reason.

## And then the trace started lying

With the wait actually waiting, the same evidence line read:

```text
just before: libkernel::sceKernelWaitEqueue(0x5e2d0000ee40) -> 0x74000086ec30
```

`sceKernelWaitEqueue` cannot return that. It returns `OK`, an errno-encoded vendor code, or the
invalid-argument placeholder - never a mapping address. And the value changed between runs
(`0x74000086ec30`, `0x74000086e9f0`) while staying inside `MAPPING_BASE`, which is what a
memory-mapping call answers.

The call record is a ring, and `RING_RETURNED[slot]` is the flag that says whether
`RING_RET[slot]` holds a real answer yet. D571 fixed the *writing* side: a call that returns
after its slot has been recycled must not store its answer there. Nothing cleared the flag when a
slot was **taken**. So a call still running in a recycled slot reported the answer of whichever
call held that slot last.

Invisible while every call returned promptly - the window was a few instructions wide. A call
that blocks for the length of a wait holds the slot open, and the defect walks straight into the
one record a person reads at a wall.

One store fixes it, ordered `Release` before the sequence is published so a reader seeing this
call's number can never still see the last call's flag. The line now reads:

```text
just before: libkernel::sceKernelWaitEqueue(0x5e2d0000ee40) from 0x400000f53413
```

No arrow, because there is no answer yet. Which is what `recorded_return`'s documentation has
promised since D459.

## The pattern, twice in one iteration

Both of these are a value that was *available* being reported as a value that was *established*.
The wait had nothing to deliver and said it had delivered; the ring had no answer and produced
one. Neither was a wrong calculation - both were a missing "I do not know yet", and in both cases
the honest state was already representable and simply not written.
