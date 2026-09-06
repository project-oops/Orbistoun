# D568 - The tail was not the tail

**Status:** measured
**Date:** 2026-09-04

## What it claimed

`CallTrace::tail` is documented as *"the last calls the guest made"*, and D154 gave it its whole
reason:

> A ranked list says what a guest spends its time on, which is the right question for deciding
> what to implement - and the wrong one entirely when something has just handed it a null and it
> died. For that, the only useful question is what it called **last**.

The printer says `last calls before the fault`. The finding that reads it calls it *"the
neighbourhood of the wall"*.

## What it is

The recorder keeps the **first** `MAX_RECORDED_CALLS` - 8,192 - and stops. `tail_of_recorded`
takes the last 48 of *those*. So for any run longer than 8,192 calls, the tail is calls
#8,144-#8,191, and the fault is somewhere far past it.

Measured across four titles:

| Title | Calls | Tail covers | Is it the end? |
|---|--:|---|---|
| PPSA28061 | 332 | #284-#331 | yes |
| PPSA02664 | 418,632 | #8,144-#8,191 | **no** |
| PPSA03416 | 467,457 | #8,144-#8,191 | **no** |
| PPSA25872 | 12,958,032 | #8,144-#8,191 | **no** |

Three of four. The dissenting one is the only title that makes fewer than 8,192 calls.

## The comment that made it invisible

`tail_of_recorded` carries this:

> for any run that stays under that - **which every run so far does, by an order of magnitude** -
> the end of it is the true end

It was true when written. It is now wrong for three of the four titles that reach anywhere, and by
three orders of magnitude for PPSA25872 rather than under by one. **A stale fact in a comment, and
the comment was the only thing standing between a reader and a wrong conclusion.**

## It already cost a wrong reading

Earlier the same day, PPSA02664's tail was read as `strlen`/`memcpy` and reported as *"the guest is
in its own code walking a structure it built, not immediately after a failed import"*. That was
drawn from **calls 8,144-8,191 of a 418,632-call run** - not the neighbourhood of the fault at all,
and no basis for any claim about what preceded it. The reading is withdrawn.

## The decision, and what is deliberately left open

**The report now says which it is.** Where the recording reaches the end it prints
`last calls before the fault`; where it does not it prints the range, the total, and
`NOT the ones before the fault`. A report claiming more than its measurement supports is the one
thing principle 3 forbids of the tools as firmly as of the emulator, and this was doing it in
capital letters on every long run.

**Decided the same day, by D571: the ring is circular now, and the tension was false.** The
beginning's only reader quotes eight entries, so a separate eight-entry record serves it while the
ring answers D154's question. What follows was the reasoning before that was noticed.

**Whether to make the ring circular is not decided here.** It is a real tension and both sides are
already written down: `MAX_RECORDED_CALLS` argues for the beginning - *"the interesting part of a
boot is its beginning; by the ten-thousandth call the guest is in a loop"* - and D154 argues for
the end. Keeping the first 8,192 and *calling* them the last is the only position that was never
defensible; the choice between them belongs to whoever weighs a boot's opening against a wall's
neighbourhood, and it is a mechanism change rather than a fix.

## What this does not establish

**That the first 8,192 are the wrong thing to keep.** They may well be the more useful half; the
finding is only that they are not what the tail claimed to be.

**Nor how many other readings rest on it.** One was found because it was made today and could be
checked. Any conclusion drawn from a long run's "tail" before now has the same flaw, and nothing
here audits the back catalogue.
