# D625 - A dump was taken and never shown

**Status:** measured
**Date:** 2026-09-08

## The third layer of the same silence

D623 found two reasons `ORBISTOUN_DUMP` produced nothing: a forced list that added to the default
set instead of replacing it, and a dropped dump that printed as absence. Both were fixed. It still
produced nothing.

The third reason is the one that had been there since forcing was added. **Dumps reach a reader
only through a finding about the same import**, and every finding that carries them is about an
import *nothing implements*. So a dump forced on an implemented import was taken, kept in the
trace, and then discarded at the last step - and the run looked exactly like one where the guest
never made the call.

Which is precisely the case forcing exists for. D198 said so when it was added:

> Imports named for a dump are dumped even though something implements them, because the case
> that matters is when the implementation is yours and you suspect it.

Collection honoured that from the first day. Reporting never did.

## Three changes, and every one of them is a sentence somebody can read

**`Gap::Captured`** - a finding that is an answer rather than a gap. One per import that has dumps
and that no other finding already speaks for, so an unimplemented import's arguments still appear
where they always did rather than twice.

**Printed ahead of the ranked six, not among them.** A `Captured` finding exists only because
somebody typed a variable naming an import they suspect. Ranking it against findings the tool
volunteered - and cutting it at six - answers a different question from the one that was put. It
carries weight zero so it can never outrank a fault in any consumer that sorts.

**The list says what it armed**, in both directions:

```text
orbistoun: ORBISTOUN_DUMP matched no import called "sceKernelNoSuchThingAtAll"
orbistoun: ORBISTOUN_DUMP armed 2 of 1040 stub slot(s)
```

The first is copied from the `ORBISTOUN_WRITE` path three lines below, which has counted
per clause since D230 and reports a clause that matched nothing. The dump path never did, so a
misspelled name and an import the guest never called produced identical silence. The second is
new to both: **a list that matched and a list that did not both produced no output**, and which
of the two happened is the entire question when a dump comes back empty.

## Made to fire, both ways

A guard nobody has watched reject something is a guard nobody knows anything about, so both cases
were run:

```text
ORBISTOUN_DUMP=sceKernelWrite            -> ! libkernel_fs::sceKernelWrite was asked about,
                                              and here is what it was passed
                                            arg1 = 0x40000022fb9c -> image+0x22fb9c =
                                              "obscene: eboot entry reached"
ORBISTOUN_DUMP=sceKernelNoSuchThingAtAll -> matched no import called …
```

The first is an implemented import, called four hundred thousand times, whose arguments this tool
could not show until today.

## And then it answered the question it was built for

With all three fixed, `ORBISTOUN_DUMP=sceKernelAllocateDirectMemory` on the payload arms its slot,
reports arming it, and captures nothing - which now means one thing and not three. The guest does
not call it. D626 says why.

Four tools in one session have reported confidently while omitting what was asked (D613's ring,
D615's surplus registers, D623's buffer, this). Each was a missing "I do not know". This one had
been missing for as long as the feature existed, and was found only because the other three were
fixed first - each fix removing one explanation until a single one was left.
