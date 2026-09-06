# 2026-09-03 - (/loop) The oscillation is one allocation round, and the backlog is now the whole list

```
runs of PPSA02664         37
outcomes                   2  (2077 calls/44 distinct, 2080/46)
hypotheses killed          4
differential             146  ->  194 cases
obSCEne backlog 022        4 entries -> 504 functions, 717 questions
```

Arrived by hand again - six scheduled wakeups now, none of which fired.

## The ±3 is a branch, and it moves the verdict again

D488 left a **±3 call oscillation** with the note that it *"does not move the verdict, which
keys on distinct imports"*. At the wall D489 moved the guest to, the distinct count oscillates
too: **2077 calls / 44 distinct**, or **2080 / 46**. Fifteen and twenty-two of thirty-seven
runs. The verdict reads `same` on one and `FURTHER` on the other, on identical code.

It is one allocation round:

| | 44-distinct | 46-distinct |
|---|---|---|
| `sceKernelMprotect` | 1 | absent |
| `sceKernelAllocateMainDirectMemory` | absent | 1 |
| `sceKernelMapDirectMemory` | absent | 1 |
| `sceKernelSetVirtualRangeName` | absent | 1 |
| `sceKernelDirectMemoryQuery` | 3 | 4 |

Exactly D488's table, one round instead of the fourteenth - **the same phenomenon that decision
half-fixed**, not a second one. Ordered by first call the two runs are identical through index
18, every import with the same count, and diverge at 19.

## Four hypotheses killed

The persisted run record (deleted before each of three runs), the retained sandbox
(`ORBISTOUN_SANDBOX=ephemeral`, six runs), uninitialised memory contents (heap, stack and direct
filled together, five runs each) - all still bimodal.

And **my own first reading**: the first six runs alternated perfectly, which reads as a
mechanism carrying state between runs. Thirty-one more runs made it a coin flip. Six samples
could not tell those apart, which is an argument for the three-sample rule being a floor rather
than a target.

## What is left is a leak the report already names

One non-deterministic input remains: D128 serves `malloc` from `std::alloc`, so **the guest
holds host heap addresses** that move under ASLR on every launch. orbistoun's own dump
diagnostic says what is wrong with them:

```text
arg5 = 0x7ff71530f030 -> no region this run mapped, and address-shaped
```

D128's reasoning - identity-mapped, so a host allocation is a guest allocation, and a private
arena buys nothing - is about correctness and it holds. It did not weigh reproducibility.

**Not built here.** The test is a heap at a fixed base, and that needs `orbistoun-libc` to
depend on `orbistoun-mem` and to register the region in the address space - a second allocator,
which D128 declined, and which done carelessly recreates the collision class D463 and D488 have
already paid for twice. That is a decision to take deliberately with this measurement in front
of it. D499.

## And the obSCEne backlog is the whole list now

`status` reports **717 open questions a hardware probe could settle**; the backlog written
yesterday held four. Joined `orbistoun-cli questions` against what obSCEne can call and tiered
it by what blocks the measurement: **137 callable today** (1.26M recorded calls), **63 needing a
signature**, **303 not even censused**. obSCEne D320.

Two of the loudest functions a guest touches - `sceKernelDebugOutText` at 220,383 calls and
`_Getpctype` at 415 in the current run - were not on obSCEne's radar in any form.

## And the ctype family goes from no coverage to exhaustive

Differential item (c) was largely stale - `strstr` repeated prefixes, `strncasecmp` edges and
`qsort` duplicates/reversed are all already covered. The real gap, found by diffing the bound
implementations against the cases: **thirteen ctype functions with no case between them**, and
this title calls into that family 415 times in a run that dies after 2,077 calls.

**146 -> 194 cases.** The eleven predicates are swept over all of `0..127` as two 64-bit
bitmaps each - a case per character would have been 1,408 of them for the simplest functions in
the file, and a bitmap samples nothing. `tolower`/`toupper` get the boundaries instead, because
their answer is a byte and `@[`/backtick-`{` are where an implementation that adds `0x20`
without a range check shows itself.

Stopped at 127 deliberately: above it the answer is a locale property, and pinning glibc's high
half here would record an assumption as the contract. obSCEne's `035-libc/getpctype` is what
settles that half.

Compared as *classified or not*, never as the raw return - the standard promises non-zero and
glibc answers a mask out of its table.

**Watched failing.** Clearing bit 1 of `isalpha/high-half` (the letter `A`) in the committed
reference gives `returned 0x7fffffe07fffffe, glibc 2.39 returned 0x7fffffe07fffffc` - orbistoun
holding the correct answer against a corrupted expectation. Restored with the same two-line
anchor. The harness already had `every_recorded_case_can_be_rebuilt`, which is why a green run
means the new cases were compared rather than skipped.

One thing to watch: the two edits I made to break and restore rewrote the reference file with
CRLF - Python's text-mode write translates on Windows - and the gate strips carriage returns
from the fresh output before diffing, so it would have failed on line endings alone.
Regenerated in place through the same pipeline the gate uses; `od` and a byte count confirm
0 CR, 588 LF, 194 cases.

**And the instrument lied about it, which is the fifth this session.** `grep -c` for a carriage
return in Git Bash reports 3 for a file just written with three LF-only lines: the `$'\r'`
escape never reaches grep. Two readings of "588 CR lines" were facts about the shell's quoting.
The control took one line - grep a known-LF file and a known-CRLF file, and watch them answer
the same number.

## State

`cargo test --workspace` green - 117 suites, **1992 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean on both repos.

Nothing committed. The day holds worklogs 292-349 and D466-D499.

**Next**: the 95 outstanding hardware measurements. The remaining differential gap after this
tick is `strnlen`, `strlcpy`, `strnstr`, `strpbrk` and `strdup`/`strndup` - all fit helpers that
already exist. The Annex K `_s` forms stay out: glibc does not provide them, so there is nothing
to diff against.
