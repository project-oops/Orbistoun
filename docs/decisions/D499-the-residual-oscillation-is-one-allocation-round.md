# D499 - The residual oscillation is one allocation round, and it moves the verdict again

**measured** - 2026-09-03 (37 runs of PPSA02664, plus four eliminations)

D487 recorded that a run of this title is not reproducible. D488 found the larger half - the
module loader had placed the title's modules on the address its own C++ allocator reserves - and
left a **±3 call oscillation** behind, with the note that it *"does not move the verdict, which
keys on distinct imports"*.

**That note is stale.** At the wall D489 moved the guest to, the distinct count oscillates too:

```text
2077 calls, 44 distinct        2080 calls, 46 distinct
```

Fifteen runs of the first and twenty-two of the second, across thirty-seven. The verdict reads
`same` on one and `FURTHER` on the other, on identical code - so the project's only progress
measure now flips on this.

**And it is not noise.** The first six runs alternated perfectly, which reads as a mechanism;
thirty-one more showed a coin flip. Six samples were not enough to tell those apart, which is
the whole reason the three-sample rule exists and an argument for making it more.

## It is a branch, and the same branch D488 tabulated

| | 44-distinct state | 46-distinct state |
|---|---|---|
| `sceKernelMprotect` | 1 call | absent |
| `sceKernelAllocateMainDirectMemory` | absent | 1 call |
| `sceKernelMapDirectMemory` | absent | 1 call |
| `sceKernelSetVirtualRangeName` | absent | 1 call |
| `sceKernelDirectMemoryQuery` | 3 | 4 |

Three calls gained, one lost: the -3. **The guest either does one more allocation round, or one
fewer and then re-protects what it already has** - exactly D488's table, one round instead of the
fourteenth, so the residual is the same phenomenon that decision half-fixed rather than a second
one.

The divergence point is exact. Ordered by first call, the two runs are **identical through
index 18 - every import, in the same order, with the same call count** - up to and including
`sceKernelGetDirectMemorySize`. At index 19 one run maps direct memory and the other does not.

## Four hypotheses killed, each by a measurement

D488 killed thread scheduling (the guest is single-threaded here) and reservation-failure
variation. Today:

- **The persisted run record.** `traces/…json` is the only file a run writes. Deleted before
  each of three runs: still bimodal.
- **The retained sandbox.** `ORBISTOUN_SANDBOX=ephemeral`, six runs: still bimodal. Worth
  killing explicitly because it is the one piece of state the emulator deliberately keeps
  between runs.
- **Uninitialised memory contents.** `ORBISTOUN_HEAP_FILL=00`, then heap, stack and direct
  memory filled together, five runs each: still bimodal. This eliminates the whole family the
  fill diagnostics exist to test.
- **Strict alternation**, which was my own first reading and would have implied state carried
  from run to run. Thirty-one further runs killed it.

## What remains, and the test that would settle it

One non-deterministic input is left: **the guest is handed host heap addresses.** D128 serves
`malloc`/`memalign`/`_Znwm` from `std::alloc`, reasoning that *"the address space is
identity-mapped, so a host allocation is a guest allocation at the same address, and a private
arena would buy nothing but a second allocator to get wrong."*

That reasoning is about correctness and it holds. What it did not weigh is **reproducibility**:
those addresses move under host ASLR on every launch, and this title's allocator does
arena-relative arithmetic on what it is given (D443). orbistoun's own dump diagnostic already
says what is wrong with them, in the run report, about the pointers it handed out:

```text
arg5 = 0x7ff71530f030 -> no region this run mapped, and address-shaped
```

**A pointer the guest holds that belongs to no region the guest was shown.**

The test is a heap at a fixed base - reserve a region, bump within it, keep D128's header layout
so `free` is unchanged for anything outside. If six runs then agree, the leak is confirmed; if
they still split, the last candidate is gone and the cause is somewhere nobody has looked.

Not built here: `orbistoun-libc` depends on `orbistoun-core` alone, and giving it
`orbistoun-mem` to reserve a fixed base is a dependency edge and a second allocator - the thing
D128 explicitly declined. That is a decision to take deliberately, with this measurement in
front of it, rather than as a step inside an investigation.

> **Built, and settled - see D513.** Two corrections to the paragraph above, both found by
> doing it. The dependency count was wrong: `orbistoun-libc` already took `env`, `fs`, `hle`
> and **`thunk`**, and `thunk` already reserves at fixed bases - so the edge this deferred on
> did not exist. And the prediction was only half right. Fixing the heap does collapse the
> split (51 runs, one branch), but **the address value is not the input** - thirteen bases
> thirty bits apart all take the same branch. Reversing the *direction* the region grows in
> flips it every time. The guest's own allocator is choosing between extending a range it
> holds and taking a fresh direct-memory segment, and the host heap decided that by where it
> happened to put things.

## What to do about the verdict in the meantime

Nothing yet, and D487's reasoning still applies: repeating each run and reporting a spread would
bake the noise in as a property of the emulator. But `FURTHER` and `same` are now separated by a
coin flip on this title, and no verdict from a single run of it should be believed.
