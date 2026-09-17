# D698 - the give-up gate is the closest call below the stop, not the most recent

**Status:** decided
**Date:** 2026-09-15

## The choice

When a guest stops itself - `abort`, `exit`, a `ud2` - the finding now names one call as the gate:
the call the giving-up code made **itself**, singled out from the recent history. The choice is which
call that is, and it is **the one on the aborting thread whose call site is closest *below* where the
stop was decided** - not the most recent call before the stop, and not any recent call answering an
error.

## Why not the most recent call

Because a guest's abort path makes calls of its own on the way down. PPSA28061 calls
`sceErrorDialogInitialize` - a call into another module - after its gate check and before `abort`. So
"the last thing it called" names the dialog, which is part of the giving-up ritual, not its cause. The
gate is the mapper call `0x4d` bytes earlier, in the same function. Closest-below the stop finds it;
most-recent does not (worklog 610, and the made-to-fail test that mutates to most-recent and fails
naming the dialog at delta `0x800008f538f`).

## Why the distance is the signal, and why it is reported not thresholded

The giving-up function calls the gate, tests the answer, and calls `abort` a few dozen bytes later, so
the gate's call site sits just below the stop's. Calls from other frames and other modules sit
gigabytes away - `0x4d` versus `0x8000_08f5_38f` in Earthion's own trace. The distance is a total
order over data the trace already holds (`from`, `thread`), needing no module-range table and no
hot-path change.

It is **reported as a delta rather than turned into a threshold** because a threshold would be an
invented constant (principle 3), and a wrong one would make the gate lie. `0x4d` reads as the same
function; a gigabyte reads as "not this decision"; the reader judges from the number. The finding
states an observation - the nearest call below the stop, and how near - not a verdict about cause,
which is what keeps it from being a new confident-wrong lead of the kind D677 and worklog 606 exist to
prevent.

## Why this exists at all

The flat "read the calls immediately before it" list let this project blame `sceAgcCreateShader -> 0x0`
for PPSA28061's abort - a call two frames back, in a different image, that returned fine - when the
cause was the mapper gate D677 read by hand. A finding that names the gate mechanically is the fault
taxonomy naming itself rather than depending on the reader to disassemble, which is the direction the
taxonomy work has been going (worklog 605, 606, 609).

## Scope, and what was deliberately left

The gate is `None` when nothing was called from below the stop on its thread - the finding then says
only what it always did, rather than reaching for an unrelated call (the negative test pins this). The
delta is a within-frame proxy for "same function": a module-range table would let the finding say
"same module" outright, but that is a producing-side change for a sharper word, not a different answer,
and is not needed for the gate to be right. Left for if a title ever needs it.
