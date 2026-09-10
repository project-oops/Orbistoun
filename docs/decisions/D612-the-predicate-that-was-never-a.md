# D612 - The predicate that was never a predicate

**Status:** measured
**Date:** 2026-09-08

## What the measurement said, and what I nearly did with it

D611 left `031-stackattr/address-is-the-base:sceKernelIsStack:is-stack` on the outstanding list
rather than asserting it, because the console answered `0` for an address inside its own stack
and orbistoun answers `1`. The note said the work was reading the probe's check rather than
writing an assertion.

Reading it took ten minutes and the answer is not a disagreement about a value.

```c
int on_stack = sceKernelIsStack((void *)&frame, &low, &high);
```

**`sceKernelIsStack` takes three arguments.** The address, then two `void **` where the bounds
go. It returns a status. Orbistoun declared arity 1, returned `1` for an address in the stack and
`0` for one outside, and wrote no bounds at all.

Every part of that is contradicted:

| | orbistoun | the console |
|---|---|---|
| arguments | 1 | 3 |
| a local | `1` | `0` |
| a static | `0` | `0` |
| bounds written | none | `low`, `high` |

The second and third rows are one measurement, and it is emphatic: `010-kernel/is-stack` fails in
**twenty-three runs out of twenty-three**, always with `a stack address and a static one were
reported alike`, value `0x0`. That check was written for a predicate, so it reads a successful
call as a broken function. It is not broken - the answer is in the out-parameters, and
`031-stackattr` records them two mebibytes apart, exactly the stack size the same run read off
the thread's own attribute.

So a guest asking this where its stack is got an inverted flag and two words of its own
uninitialised memory back.

## Why it survived

The name. `IsStack` reads as a question with a yes-or-no answer, the implementation answered one,
and every test in this repository asserted the same reading - so the tests agreed with the code
about something neither had checked. The measurement that contradicts it has been in the corpus
for as long as the corpus has existed and was **read as evidence the console's function was
broken**, which is what the knowledge entry said in as many words.

That is the shape worth naming: a measurement that contradicts an implementation was filed as a
fact about the platform's defect rather than about ours. The check's own failure text is what
made it plausible - obSCEne said "reported alike", which sounds like a complaint about the
console.

## The change

Arity 3. The bounds are written through the second and third arguments when they are non-null,
and the return is `0`.

**One thing is deliberately not distinguished.** Whether the console writes the bounds when the
address is *outside* any stack is not measured: it answers `0` either way and no capture records
the two words for the static case. They are written here regardless, because they describe the
calling thread's stack rather than the address - a caller that asked about a static still learns
where its own stack is, and refusing to say would invent a distinction nothing has measured.

## Two harnesses caught it, which is the part worth keeping

The test helper in `tests/posix.rs` fills unspecified argument registers with
`0xDEAD_BEEF_DEAD_BEEF` rather than zero, precisely so a function reading an argument it was not
given is caught. It caught this immediately: the new implementation faulted writing through it.
A helper that padded with zeroes would have let the change land silently and written nothing.

And `declared_arity_and_recorded_arity_never_disagree` refused the change until the knowledge
entry moved with it, so the arity cannot be right in one file and wrong in another.

## It moved the wall, and that was established by taking it out again

The title that has been this project's wall reached 193 distinct imports and faulted at
`image+0x1389269`. With this change it reaches 196 or 197 and faults at `image+0x39f7c`.

An intervention that moves a wall is not a diagnosis, so it was taken out and put back:

| `sceKernelIsStack` | distinct imports | fault |
|---|--:|---|
| as a predicate, one argument | 193, 193 | `image+0x1389269` |
| three arguments, bounds written | 196, 197, 197 | `image+0x39f7c` |

Reverting the body alone - leaving everything else this session changed in place - puts the guest
back on the old number and the old fault site exactly. Restoring it brings the new ones back.
Two observations each way, and no other change in the tree accounts for either.

The event-flag mode refusal was tested the same way and is **not** responsible: with mode `0x00`
reading as `or` again the fault stays at `image+0x39f7c`. It is on the guest's path - 70 calls to
`sceKernelWaitEventFlag`, 9 to `sceKernelPollEventFlag` - and it moves nothing here.

**Where it now stops is not "further" in the ordinary sense**, and the report is careful to say
so: `0x39f7c` is a *lower* offset than `0x1389269`, so the guest is dying earlier in the image
while reaching more of the interface. The two positions are in different code and do not compare.
What compares is the interface: three more of it, every run.

`image+0x39f7c` is also where PPSA02664 stops. Two titles, one offset.
