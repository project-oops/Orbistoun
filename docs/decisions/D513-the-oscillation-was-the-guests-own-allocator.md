# D513 - The oscillation was the guest's own allocator, and the deciding input was ordering

**measured** - 2026-09-03 (69 guest runs, 13 bases, both directions)

D499 measured that a run of PPSA02664 makes either 2077 calls across 44 distinct imports or
2080 across 46, that the two are internally consistent, and that it is a **branch rather
than noise**. It killed five candidates - a persisted trace, a retained sandbox, and
uninitialised heap, stack and direct memory - and left one:

> `malloc` is served from `std::alloc` (D128), so every pointer the guest receives is a host
> address, and the host randomises its layout on every run.

That is now settled, and the answer is sharper than the candidate was.

## The instrument

`ORBISTOUN_HEAP_BASE` serves every allocation from a bump region reserved at a fixed
address, instead of from the host heap. It is off by default, it is an `Intervenes`
diagnostic, and it reports what it served. The block it hands back is the same shape the
host path builds - `total` bytes aligned to `align`, carrying D128's header - so `free`,
`realloc` and `header_of` are written once and do not know which allocator answered.

**The dependency edge D499 priced into this decision does not exist.** D499 called the work
"a decision for the user, not an assumption" partly because it would cost `libc -> mem`.
`orbistoun-libc` already depends on `orbistoun-thunk`, which already reserves at fixed bases
through `AddressSpace`; taking `mem` directly is the spine's own order (principle 6) and
principle 4 requires it, since reservations belong to `orbistoun-mem` and nowhere else.
Recorded because it materially changes the reasoning that deferred this for weeks.

## The result, in three steps

**One: fixing the heap collapses the oscillation.** Twelve interleaved pairs:

```text
host heap    9 of 12 runs -> 2080 / 46      3 of 12 -> 2077 / 44
fixed heap  12 of 12 runs -> 2077 / 44      0 of 12 -> 2080 / 46
```

**Two: the address value is not what does it.** Thirteen bases, from `0x1000_0000_0000` to
`0x5E2C_0007_0000`, varying bits 16 through 45 - **fifty-one runs, every one 2077**. So the
guest is not hashing, bucketing or masking a pointer. That is a negative result, and it is
the one that made the third step worth taking.

**Three: reversing the *order* flips the branch, and nothing else changes.** The region
grows downward instead of upward. Same base, same span, same determinism, same contiguity,
same never-reused blocks - only later addresses now sit below earlier ones. Six interleaved
pairs:

```text
ascending   2077  2077  2077  2077  2077  2077
descending  2080  2080  2080  2080  2080  2080
```

Zero crossover, both directions, interleaved so the control and the measurement are in the
same session (check 7).

## What the guest is actually doing, which is the part worth keeping

The two branches are not "more progress" and "less". Diffing the persisted traces:

```text
only in 2080   sceKernelAllocateMainDirectMemory, sceKernelMapDirectMemory,
               sceKernelSetVirtualRangeName, and a fourth sceKernelDirectMemoryQuery
only in 2077   sceKernelMprotect
```

**That is a heap deciding whether it can grow in place.** When the block it receives sits
above the range it already holds, it extends that range - one `mprotect`. When the block
sits below, it cannot, so it takes a fresh direct-memory segment, maps it, and names it.

The host allocator settles that relationship differently on different runs, because it
reuses freed blocks and can return one below a block it handed out earlier. So the guest's
allocator took a different path depending on where Windows had last put something, and the
whole ~40/60 split was that.

## Two consequences, stated because neither is obvious

**The FURTHER/BACK verdict was reporting an allocator strategy as progress.** The 2080
branch reaches three imports the 2077 branch never does, so the run report calls it
`FURTHER` - "executed code it could not reach before" - when nothing about the guest got
further. It reached *different* code. Both branches fault at the same instruction, on the
same read of `0x8`. D488 recorded that the oscillation does not move the verdict; against a
fixed heap in both directions it plainly does, and the earlier observation was made when
only one of the two could be produced on purpose.

**`-down` is now the cheapest way to exercise the direct-memory path.** Three functions and
a fourth query that no ascending run reaches are one environment variable away, on a title
that otherwise never asks for them. That is worth more than the diagnostic it was built as.

## What this does not establish

It does not say which branch the console takes. Both are paths the guest has, and the
platform's own allocator decides between them by its own rule, which is not measured. What
orbistoun now has is the ability to **choose one deterministically**, which is what every
later experiment against this wall needed and did not have.

It also does not make the run deterministic in general. `scePthreadSelf` still answers a
host address that differs between runs; it is not what decides this branch - fifty-one runs
say so - but it is a second place a guest could read the host's layout, and it is untouched.

## The base was not a free choice, and the first one was wrong

The first attempt took `0x7400_0000_0000` on the reasoning that it was clear of the module
base and the thunk table. It is `orbistoun_kernel::MAPPING_BASE`, which
`sceKernelReserveVirtualRange` hands out from. Sixteen reservations that would have
succeeded were refused, and the run fell from 2077 calls to 219 with a fault in a different
module.

Two things follow. The default moved into `0x0000_5E2*_0000_0000`, the family this project
already keeps for regions of its own invention - sentinel, content, unserved-global, poison,
described-object - which is the range nothing else claims.

And the map is no longer implicit. It was **twenty bases across nine crates**, discoverable
only by grep, with nothing to check before taking one - so `docs/ADDRESS_MAP.md` now lists
every one with its owner, and `orbistoun-service/tests/address_map.rs` walks the tree and
fails if a base is missing from it, named in it and gone from the tree, listed at the wrong
value, or **within four gibibytes of another**.

That last arm is the one that matters, and it was watched refusing the exact mistake:

```text
DEFAULT_BASE at 0x740000000000 and MAPPING_BASE at 0x740000000000 are 0 bytes apart
```

This is not the prose gate D510 argued against. A base is a constant with a name and a
value, so "the document lists every base the source declares" is a claim about two
machine-readable sets. What the gate cannot check is that a *span* fits - the map records
starts, not lengths - and that is said in the test rather than left to be found.

## And the intervention summaries were invisible on the path that matters

`heap_fill_summary` and `direct_fill_summary` were printed from `summarise_calls`, which
only two endings reach - a timeout and an exhausted budget. **A fault reached neither.** So
every intervening diagnostic this project has was silent on the most common ending it has,
which is the ending they exist to explain. Moved to `persist`, which every ending comes
through, and which already carries the reservation-failure line for exactly that reason.

Found because the first fixed-heap run printed no summary at all and the missing line read
as "the region was never built", when the region had been built and had broken the run.
