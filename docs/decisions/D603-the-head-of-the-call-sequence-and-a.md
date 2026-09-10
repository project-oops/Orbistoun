# D603 - The head of the call sequence, and a redirection that measured nothing

**Status:** measured
**Date:** 2026-09-08

## The trace kept the tail and nothing kept the head

Every "what happened before X" question this project asks has had to be answered by inference.
The trace keeps the **tail** of the call sequence, which exists for the fault - what was called
just before it died. Nothing kept the opening.

There was an opening record, and it held **eight** entries with no call sites, sized for the halt
summary that quotes eight (D571). It is two thousand and forty-eight now, and each entry carries
the address the call was made from - the same argument D596 makes for a formatted message: a name
says what ran, an address says which code ran it.

The cost is what it always was: a store per call while the count is below the ceiling, and a load
after. Two thousand stores in a run of four hundred and sixty-seven thousand is not a sink that
changes what it observes; keeping every call would be.

## It corrected a reading in its first use

The mapping record indexes by call ordinal, and the drift had been localised to "a mapping at
call 226 that appears in some runs" (D602). With the sequence readable:

```text
218  sceKernelReserveVirtualRange
223  sceKernelVirtualQuery
224  sceKernelAllocateMainDirectMemory
225  sceKernelMapDirectMemory
226  sceKernelSetVirtualRangeName
```

**`at_call` is a call *count*, not a call index** - it is `total_calls()` at the moment the
mapping was placed, which its own documentation says. So "the mapping at call 226" is the map at
call **225**, and the field one past it names a different function entirely. The record was
right and the reading was off by one, which only a list of the calls could show.

## And it exposed a measurement that never happened

D602 concluded that no run records a refusal and the map is *never attempted*. Both halves came
from runs captured as `2>&1 > file`, which duplicates the error stream to the terminal and then
redirects only stdout - so every grep for refusals searched output that contained none of them.

Captured as `> file 2>&1`, every run records **two to four refusals**, and each answers
`0x7fff0004` - `NoMemory`. The map is attempted, and orbistoun declines it because the
reservation conflicts.

The refusal record itself was also too narrow: it started on the one failure path anybody had
looked at, and `map_named_direct_memory` has a dozen. Every non-success exit is recorded now,
from one place at the top of the function rather than at each `return` - the multi-site hazard
this project has paid for three times.

**A shell mistake produced a confident, wrong entry**, and it is worth naming as its own failure
mode: the tooling was correct, the conclusion was drawn from an empty file, and nothing about the
output looked wrong - `refusals=0` is exactly what a working measurement of a run with no
refusals prints.

## What this does not establish

**Why the conflicts vary.** Two runs record two refusals and one records four, and the import
count follows. What makes a reservation conflict in one run and not the next is the same
unanswered question one level down.

**Nor that the head is long enough.** Two thousand reaches call 225 comfortably and would not
reach a divergence at call ten thousand. The ceiling is a guess against the one case that needed
it.
