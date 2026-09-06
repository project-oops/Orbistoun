# D523 - The thread affinity setter, and the one beside it that stays unimplemented

**guest-observed** - 2026-09-03 (arities from the arguments a run passes; six samples after)

Two functions, called 94 times between them, both answering a placeholder. One is now
implemented and one deliberately is not, and the difference between them is the whole point of
writing this down.

## The arities came from the run, not from a header

Neither is declared anywhere in orbistoun, and neither is in the POSIX crate, so there was no
in-project arity to cite. The run report prints the arguments an unimplemented call received,
which is how D516 fixed `sceKernelCreateEqueue` at two:

```text
scePthreadSetaffinity    arg0 = 0x1d1d9000960   arg1 = 0x1ffb    (and 0x1 on another call)
                         arg2.. address-shaped leftovers
scePthreadGetschedparam  arg0 = 0x1d1d4b785e0
                         arg1 = 0x6000007fc88c -> stack   arg2 = 0x6000007fc888 -> stack
                         arg3 = 0x63a6d, arg4, arg5       leftovers
```

`Setaffinity` takes a subject and a small bitmask - **arity 2**, the same shape as
`scePthreadAttrSetaffinity`, which orbistoun already declares at two. `Getschedparam` takes a
subject and **two writable stack addresses four bytes apart** - arity 3, two four-byte
out-parameters, with the registers after them holding leftovers.

## `scePthreadSetaffinity` is accepted, and the bargain is the attribute form's own

`pthread_attr_setaffinity` already states it: which host core a guest thread runs on is the
host scheduler's to decide, and orbistoun does not pin guest threads. That is a cited
convention rather than a new judgement.

**The difference from the attribute form is real and is stated rather than glossed.** The
attribute form owns a block, so it can hand back what a caller set. This one has nowhere to put
the mask, so a guest that set an affinity and read it back would not get it. Nothing observed
does - the 62 calls never read one back, and there is no `scePthreadGetaffinity` in the import
table - but the moment something does, this needs a per-thread record and not a wider `Ok`.

Answering `Ok` rather than the placeholder is the point: a caller testing a scheduling call
against zero reads `0x7fff_0001` as *"the affinity was refused"*, which is a lie in the
direction that stops a guest. Nothing here claims the mask was applied; a setter's contract is
that the request was taken.

## `scePthreadGetschedparam` stays unimplemented, on purpose

Its arity and its two out-parameters are established. What it should *write* is not.
**Orbistoun keeps no per-thread scheduling record**, so a policy and a priority handed back
would be invented - and unlike `sceVideoOutIsFlipPending`, where the answer followed from a
model this project had already written down (D516), there is no model here to derive one from.

The attribute form can answer because it stores what a caller set. The thread form has nothing
to answer from. That is the whole difference, and it is why one of these two is code and the
other is a knowledge entry saying why it is not.

Recorded in the knowledge file with the reason, so the next reader finds the decision rather
than the gap.

## The cost: the call count is no longer exact

Before, three runs gave 415,415 calls every time. After, six runs give 415,355 to 415,357.

```text
415355  415355  415356  415356  415356  415357
```

**Distinct imports stay at 181 in all six, and the fault is identical** -
`read of 0xa0 at 0x400001389269` - so the `FURTHER`/`BACK` verdict, which keys on distinct
imports, is unaffected. The raw call count is now +/-2.

The likely cause is that the guest creates 32 threads and a scheduling call it now sees
succeed changes how long something spins. Stated as likely, because it has not been measured -
what *has* been measured is which numbers moved and which did not, across six samples.

D488 once claimed an oscillation "does not move the verdict" and was wrong because the distinct
count moved with it (D519). This time the distinct count was checked, six times, and it did
not.

## And the guest reached something new

`sceKernelAddUserEventEdge`, two calls, which no previous run has ever made. Small, and the
kind of thing that only shows up because the stub above it stopped answering an error.
