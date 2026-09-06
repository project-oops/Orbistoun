# 420. The ring is circular, and the trade was false

**2026-09-04** - directed

## What was done

Made the recorded-call ring circular, so `CallTrace::tail` is the last calls the guest made rather
than the last 48 of the first 8,192 (D568).

| Title | Calls | Tail before | Tail now |
|---|--:|---|---|
| PPSA02664 | 418,253 | #8,144-#8,191 | **#419,560-#419,607** |
| PPSA25872 | 7,004,897 | #8,144-#8,191 | **#7,053,264-#7,053,311** |

**D568 called this a trade and it was not one.** The ring's opening has exactly one reader - the
halt summary - and it quotes **eight** entries. So the opening now lives in its own eight-slot
record that is never overwritten, and the ring answers the question D154 actually asked. Both
sides, for the price of eight atomics. There was never a choice to make, only a check nobody had
run.

## It paid immediately

PPSA25872's tail is now **48 of 48 calls to the unnamed futex** - the function that is 78% of every
call this project has ever recorded (D566). Before, that field showed early-boot noise from call
eight thousand, and nothing in the report connected the wall to what the guest was actually stuck
on.

## What wrapping costs

Three readers silently assumed a slot's position *was* its sequence. Wrapped, slot 3 may hold call
3 or call 8,195. So a sequence is stored beside each entry, `recorded_calls` sorts by it,
`last_call` takes the highest sequence rather than the highest slot, and - the one failure a
circular ring can cause that a filling one cannot - **a return is written only if the slot still
belongs to that call**, since after a wrap a later call owns it and the answer would be filed
against the wrong function.

## And the classification still fails

This was done to unblock classifying the 97. Six titles re-run with the window at the fault **and**
all six arguments recorded: **one producer attributable, the same one as before.**

Third explanation tested and refuted - `arg0`-only, then the window, and what is left is the plain
reading: **the guest does not pass these answers on.** It stores them, tests them, or drops them.
For a function called once whose answer is ignored there is no evidence anywhere and never will be
from a run.

The classification is not blocked on tooling. It is blocked on the guest not caring, and three
measurements now say so rather than one guess.

## Worth noting about the day

The `reaches_end` warning added this afternoon is now effectively unreachable - it fired for
PPSA02664 two hours ago and cannot fire again while the ring is circular. Kept, because what it
would catch is a regression to a filling ring, and it has been watched failing. A guard that has
never fired is the one to distrust; this one has.
