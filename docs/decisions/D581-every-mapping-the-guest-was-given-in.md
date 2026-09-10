# D581 - Every mapping the guest was given, in order

**Status:** measured
**Date:** 2026-09-08

## The half a run has never reported

A run says this and nothing else about memory:

```text
orbistoun: 1 reservation(s) failed, first at 0x7400047e0000, len=0x1fe0000 - conflict
```

The **failures**. Nothing about the eighty-odd that succeeded - so a pointer into guest memory
could not be traced back to the call that produced it, and two runs whose arena addresses
differed could be seen to differ and not where.

That is the asymmetry D578 closed for the filesystem, in the second subsystem that had it.
`ORBISTOUN_TRACE_MAPS` is `ORBISTOUN_TRACE_OPENS` for memory, gated for the same reason: a
title maps steadily for as long as it runs, so an ordinary run pays one atomic load.

## Four fields, and three of them exist because a diff needed them

| field | why |
|---|---|
| base, length, readable | what was placed |
| **order** | a bump allocator's address is a function of everything before it, so the list is a sequence and not a set |
| **`at_call`** | *when*, in the run's own units. An address says two runs differ; a call ordinal says where to look |
| **`during`** | *what was running*. The import last called, named by the reporting layer, which is the one that holds the table |
| **`hinted`** | a guest that names its own address repeats; one taking what the arena offered moves when anything before it moves |

Each of the last three was added because the previous diff could not answer the next question,
and the third is what ended the investigation in one run: mapping 19 in one run said
`during libkernel::scePthreadCreate`. **The guest makes threads**, which nothing in the default
report says because `scePthreadCreate` is implemented and only unimplemented calls are ranked.

## What it measured

With the two determinism fixes of D582 in place, two runs of PPSA03416:

| | identical mappings |
|---|---|
| host clock | **1** of 49 |
| logical clock | **19** of 47 |
| logical clock, ignoring call ordinals | 24 to 30 of 47 |

and the divergence sits exactly at the first `scePthreadCreate`. Before it the sequence is
identical, address for address and call for call; after it, guest threads are real host threads
and their interleaving is not this project's to reproduce.

**That is a diagnosis rather than a fix**, and the record is what makes it one: without the
sequence the same runs said only that the arena moved.

## Recorded at one site, printed through one function

`mapping_placed` already existed as the single place that means *a mapping now exists* (D579),
so the record has one call site rather than three - and the print goes through
`what_the_guest_asked_for`, the function that exists because a reporter had been wired into
some of the ways a run can end and not all of them, three times.

## What this does not establish

**That the list is every mapping.** It holds what passed through `mapping_placed`; a path that
places memory some other way is absent and would look identical to no mapping at all. None is
known, and none has been looked for.

**Nor that a stable mapping sequence means a stable run.** It is one observable of many. Two
runs agreeing here can still differ in what they compute, and the call ordinals already show
they differ in how many calls fall between mappings.
