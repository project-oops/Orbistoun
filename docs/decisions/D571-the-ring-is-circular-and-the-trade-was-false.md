# D571 - The ring is circular, and the trade it seemed to require was false

**Status:** measured
**Date:** 2026-09-04

## What D568 left open

The recorded-call ring filled once and stopped, so `CallTrace::tail` - *"the last calls the guest
made"*, printed as `last calls before the fault`, and given its purpose by D154 as **the
neighbourhood of the wall** - was calls #8,144-#8,191 of runs making four hundred thousand and
twelve million.

D568 declined to fix it, on the grounds that it was a trade with two written-down sides:
`MAX_RECORDED_CALLS` argues for the opening (*"the interesting part of a boot is its beginning"*)
and D154 argues for the end.

## The trade was false

**The opening has exactly one reader, and it quotes eight entries.** The halt summary takes
`SUMMARISED_CALLS = 8` and prints *"first imports called"*. Nothing else in the tree reads the
beginning of the ring at all.

So the ring is circular now, and the opening is kept in its own eight-slot record that is never
overwritten. Both questions are answered; the cost is eight atomics. There was never a choice to
make - only a check nobody had run.

## What wrapping costs, and where it is paid

Three readers assumed a slot's position *was* its sequence, which was true while the ring filled
once. Wrapped, slot 3 may hold call 3 or call 8,195:

- **A sequence is stored** beside each entry, so a wrapped slot says which call it holds.
- **`recorded_calls` sorts by sequence**, because slot order is now the order the buffer happens to
  sit in and says nothing about the guest.
- **`last_call` takes the highest sequence**, not the highest slot - the newest call can be
  anywhere in the buffer, and taking the last slot would name whichever function landed there.
- **A return is recorded only if the slot still belongs to that call.** After a wrap a later call
  owns the slot, and writing the answer there would attribute it to the wrong function. This is
  the one failure a circular ring can cause that a filling one cannot, and it is guarded by
  comparing the stored sequence before the write.

## Measured

| Title | Calls | Tail before | Tail now |
|---|--:|---|---|
| PPSA02664 | 418,253 | #8,144-#8,191 | **#419,560-#419,607** |
| PPSA25872 | 7,004,897 | #8,144-#8,191 | **#7,053,264-#7,053,311** |

It paid immediately. PPSA25872's tail is now **48 of 48 calls to the unnamed futex** - the function
D566 identified as 78% of every call this project has ever recorded. Before, the same field showed
early-boot noise from call eight thousand, and nothing in the report connected the wall to the
thing the guest was actually stuck on.

## And the classification still fails

The change was made to unblock classifying the 97 unimplemented functions with no `returns`, by
seeing what a guest does with each tagged placeholder. Six titles re-run with the window now at the
fault **and** all six arguments recorded (D570):

**One producer attributable. The same one as before.**

That is the third explanation tested and refuted - `arg0`-only (D570), the 8,192-call window
(here), and what remains is the plain reading: **the guest does not pass these answers on at all.**
It stores them, tests them, or drops them. For a function called once whose answer is ignored,
there is no evidence anywhere and never will be from a run.

The classification is not blocked on tooling. It is blocked on the guest not caring.

## What this does not establish

**That nothing was lost.** The ring no longer holds calls 9 through 8,191 of a long run, and no
reader wanted them today. One might tomorrow, and it would have to say so and get its own record.

**Nor that the sequence store is free of races.** A reader running while the guest records can see a
slot mid-wrap - a populated index beside a sequence from the call that is replacing it. The window
is one store wide, the readers run after the guest has stopped except for `last_call` in a fault
handler, and the existing code already accepted the same shape. It is bounded, not absent.
