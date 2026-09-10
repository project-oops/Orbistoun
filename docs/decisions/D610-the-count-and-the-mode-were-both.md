# D610 - The count and the mode were both ignored

**Status:** measured
**Date:** 2026-09-08

## Two functions, four wrong answers

Reading every capture rather than five of them (D609) brought `016-syncbounds` into the
measurement table for the first time. Eight measurements, and orbistoun disagreed with four.

### `sceKernelPollSema(semaphore, need)` ignored `need`

It called `semaphore_wait(handle, Never)`, which took **one**, whatever was asked for.

| asked | available | console | orbistoun |
|---|---|---|---|
| 0 | 0 | `0x80020016` invalid | `0x80020010` busy |
| 1 | 1 | ok | ok |
| 2 | 1 | `0x80020010` busy | **ok**, having taken the one |
| 2 | 3 | ok | ok, having taken one of three |

The third row is the damaging one and the fourth is quietly wrong too. A caller told it holds
two when it holds one goes on to release two, and from there the count runs away upward - which
surfaces much later, somewhere else, as a semaphore that never blocks.

`take` is all-or-nothing now, which is what makes it a counting semaphore rather than a queue: a
caller that asked for two and received one has no way to say so and no way to give it back.
`sceKernelWaitSema` takes the same count for the same reason - nothing measured it, because a
check that blocks forever is not something a conformance probe can run, but the two calls differ
in whether they wait and in nothing else.

### `sceKernelPollEventFlag(flag, pattern, mode, …)` read one bit of the mode

It tested `mode & 0x01` and treated everything else as `or`.

| mode | console | orbistoun |
|---|---|---|
| `0x00` | `0x80020016` invalid | **ok** |
| `0x01` and | `0x80020010` busy | busy |
| `0x02` or | ok | ok |
| `0x11` and \| clear | `0x80020010` busy | busy |

`0x00` names neither `and` nor `or`, and the platform refuses it. Reading it as `or` was right
in three cases out of four **because the wrong branch usually produces the right answer**: a
pattern that fails an `and` normally satisfies an `or`, so nothing anybody tried could tell the
two apart. That is the shape of bug this project's whole measurement apparatus exists to find.

`wait_mode` answers `Some(true)` for `and`, `Some(false)` for `or`, and `None` for a mode naming
neither - including `0x03`, which names both and which **nothing has measured**. Refusing it is
the honest answer rather than picking whichever the first test happens to exercise.

## The order the checks happen in is also measured

Fixing the mode broke `measured_refusals`, and correctly: the console answers `0x80020003` to a
poll on a handle it never issued *even when the mode is invalid as well*.

So two measurements together fix something neither states on its own - **the handle is looked at
before the mode**. Validating the mode first is defensible in isolation and gets exactly one of
the two cases wrong. `sync::event_flag_exists` exists so the order can be written down rather
than emerging from how the code happens to be arranged.

The semaphore calls follow the same order, and that part is consistency rather than measurement:
nothing has measured a bad handle together with a count of zero, and it says so where it is done.

## Three tests changed their expectations, which needed saying

`tests/posix.rs` polled event flags with `mode = 0` throughout and called `sceKernelPollSema`
with the count argument omitted. Both were reasonable against the old implementation and are
argument errors on the console, so each call now names its mode and its count explicitly, with
the reason at the call.

Changing a test's expectation to match new behaviour is the move that hides a regression, so it
is worth being precise about which direction the evidence points: the console measured these,
this repository did not, and the tests were written before the measurements existed.

## What it cost to find

Nothing. The measurements have been in the sibling project's captures the whole time; what
changed is that the generator read the directories they were in. Two of the four wrong answers
were in code with a comment explaining why it was right.
