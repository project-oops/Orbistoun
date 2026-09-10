# 445. The head of the sequence

**2026-09-08** - directed, continuing 444

## What was done

**`ORBISTOUN_TRACE_CALLS` lists the run's opening calls, in order, with the address each was made
from** (D603). The trace kept the *tail* - what ran before the fault - and nothing kept the head,
so every "what happened before X" question had to be answered by inference.

An opening record existed and held **eight** entries with no call sites, sized for the halt
summary. It holds two thousand and forty-eight now, each with its call site, and the position is
the call ordinal - so it lines up with the mapping record without anybody counting.

## It corrected a reading in its first use

```text
218  sceKernelReserveVirtualRange
223  sceKernelVirtualQuery
224  sceKernelAllocateMainDirectMemory
225  sceKernelMapDirectMemory
226  sceKernelSetVirtualRangeName
```

The mapping record's `at_call` is a call **count**, not an index - `total_calls()` at the moment
of placement, exactly as its own documentation says. So "the mapping at call 226" is the map at
call **225**, and the ordinal one past it names a different function. The record was right; the
reading was off by one, and only a list of the calls could show it.

## And it exposed a measurement that never happened

D602 concluded that no run records a refusal and the map is *never attempted*. Both halves rested
on runs captured with `2>&1 > file` - which sends the error stream to the terminal and redirects
only stdout, so every grep searched a file containing none of it.

Captured properly, **every run records two to four refusals, each answering `0x7fff0004`**:
`NoMemory`. The map is attempted and orbistoun declines it, because the reservation conflicts.
D602 is corrected in place.

The refusal record was also too narrow - it covered the one failure path anybody had looked at,
and that function has a dozen. Every non-success exit is recorded now, from one place at the top
rather than at each `return`.

## Surprises

- **A shell mistake produced a confident, wrong decision entry.** The tooling was right, the file
  was empty, and nothing looked wrong: `refusals=0` is exactly what a working measurement of a
  run with no refusals prints. Worth naming as its own failure mode.
- **The drift is not a guest branch after all.** It is how many reservation conflicts a run has,
  and the guest doing less after a refused mapping is the ordinary consequence rather than a
  second mystery.

## Next

- Why a reservation conflicts in one run and not the next. The same unanswered question, one
  level down, and now the only one on this thread.
- The rest of the session's turn conclusions, still to be re-derived.
- The clean title's wall.
