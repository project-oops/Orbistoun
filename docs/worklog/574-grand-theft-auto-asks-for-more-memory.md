# 574. Grand Theft Auto asks for more memory than the console has

**2026-09-15** - the second of the two titles below its record, and unlike the first it explains
itself completely

## The gap, and that it is not worklog 555's gap

`PPSA04263` records 70 imports and 30,261 calls at `image+0x196b91a` (2026-09-07). Runs today give
**49 and 20,469** at `image+0x2bfab2f`, at the same 20-second limit. Worklog 555 took the other
title in this pair, `PPSA25872`, as far as it could go and stopped: its record does not reproduce
and the cause is outside anything this repository can reach.

This one reproduces exactly.

```
ORBISTOUN_RETURN=sceKernelAllocateMainDirectMemory:0x0
  -> 70 imports, 30,261 calls, fault image+0x196b91a
```

Every one of those three is the record, to the digit. So the record is sound, the run that made it
was not lucky, and the whole difference is one refused allocation.

## What the refusal was

The fault is four bytes after the call that causes it:

```
just before: sceKernelAllocateMainDirectMemory(0x120f00000) -> 0xf7ff0004 from image+0x2bfab2b
fault                                                                        image+0x2bfab2f
rax=0xf7ff0004
```

`0xf7ff0004` is `GuestError::NoMemory`, still in `rax` where the guest never looked at it. The
title asks for `0x1_20F0_0000` - 4.51 GiB - and the pool is `0x1_4000_0000`, 5 GiB. It looks like
a request that fits.

**It does not fit, because the guest has already taken four gibibytes.** The refusal now says so
itself:

```
refused 0x120f00000 at alignment 0x200000 (asked 0x200000) - largest placeable span is
0x3ea00000, 0x3ebb0000 free in total, across 7 region(s): 0x0..0x10000 taken,
0x10000..0x32010000 taken, 0x32010000..0x32450000 taken, 0x32450000..0xf6050000 taken,
0xf6050000..0xf6850000 taken, 0xf6850000..0x101450000 taken, 0x101450000..0x140000000
```

Five taken regions above `RESERVED_LOW`, and the trace names them: `sceKernelAllocateDirectMemory`
four times and `sceKernelMapFlexibleMemory` once. The guest reads `sceKernelGetDirectMemorySize`
**eight** times, takes 4.02 GiB, and then asks for 4.51 GiB more. Total demand is about **8.5 GiB
against a five-gibibyte pool**, so the refusal is arithmetically correct.

## What this cost to find, and what it says about the message

Every step above came from one `eprintln` that did not exist this morning. Before it, the run
reported `NoMemory` and nothing else, and a refusal is at least three different problems - a full
pool, a fragmented one, and an alignment nothing can place - that want three different fixes.
CLAUDE.md already says a message naming a cause must come from the branch that determined it;
this was a branch naming no cause at all.

Added, in `orbistoun-kernel`:

- `DirectMemory::largest_free_at(align)` - the largest span still placeable at an alignment, as
  distinct from `available()`, which sums free bytes and so answers a question no allocation asks.
- `direct::describe_regions(regions, most)` - the region list, bounded, and **counting** what it
  left out rather than truncating silently.
- The refusal itself, reporting length, both alignments, both totals and the map.

Both new functions were mutation-checked: `largest_free_at` with its alignment replaced by one
(fails), `describe_regions` with its bound raised past the list (fails).

## What was ruled out, by measurement

Recorded because each of these was a plausible half-hour:

- **The map shape.** `ORBISTOUN_MAP_SHAPE=reserved-low` forced explicitly gives the identical
  refusal. Not the shape.
- **The alignment.** The guest asks `0x200000` - two mebibytes, entirely ordinary. An alignment
  above about `0x1F10_0000` would make the request unplaceable whatever the free space, and that
  was the leading hypothesis until the message printed the real value.
- **The argument layout.** `allocate_main_direct_memory` documents its arguments past `arg0` as
  *assumed* - never separately verified - so a wrong layout was a live candidate. A two-mebibyte
  alignment in `arg1` is exactly what the assumed layout predicts, so this run is weak positive
  evidence **for** it rather than against.
- **The allocator.** `the_largest_allocation_a_title_asks_for_fits_the_default_pool` puts the
  title's own length through `allocate_aligned` on a fresh pool and passes. The fitting logic is
  not wrong; the pool really was full.
- **A shrinking pool.** `DIRECT_MEMORY_SIZE` was the obvious suspect - D398 replaced an assumed
  8 GiB with a measured 5 GiB, which would explain a record made against a larger machine.
  **Refuted by `git log -S`: the constant has been `0x1_4000_0000` since the initial commit on
  2026-09-01**, six days before the record. It was 5 GiB when the record was taken.

## What is actually open

The title runs on hardware, and on hardware the pool is the same five gibibytes this measured.
So one of these is true, and nothing here yet says which:

1. **`Main` is a different pool.** `sceKernelAllocateMainDirectMemory` and
   `sceKernelAllocateDirectMemory` are modelled as one range. If the vendor separates them, every
   number above is being drawn from the wrong account.
2. **The earlier four allocations are too large**, because something answered wrongly upstream -
   the guest sizes them off eight `GetDirectMemorySize` calls, so a wrong answer there scales
   straight into them.
3. **The guest frees between them and the release is not landing.**

Not guessed at. (1) is the one worth measuring first and it is a question for a probe, not for
this repository: two allocations and two `GetDirectMemorySize` calls on hardware would settle
whether the accounts are separate.

## Surprises

**The record was right and the run was right.** The instinct on a number that fell was that
something regressed. Nothing did: the emulator refuses correctly, the title asks correctly, and
the two are incompatible because a fact about the console is missing. Worklog 555's "weak form"
said a guest doing more correct things can score worse; this is the same shape one level down -
the guest gets far enough to *allocate*, and allocating is what kills it.

**A discrepancy I reported and then had to withdraw.** The trace records `memory_map` as two
regions, wholly free, for the very run whose refusal listed seven regions with four gibibytes
taken, and this entry first said so as an open puzzle - suggesting `queried_map` and D357 needed
re-checking.

They do not. `memory_map` is written by `record_conditions` inside `prepare_diagnostics`, which
runs **before the guest does** - "what this run is subject to, recorded before anything can
fault". It is the map *as constructed*, and its purpose is to catch a shape that fell back
because its regions did not fit (D357). A pristine pool is the correct and only possible value,
and every title's trace carries the identical one, which is what checking four of them showed.

The real lesson is the one that produced the false alarm: a field named `memory_map` sitting in a
report about a run reads as the map the run *used*. It is the map the run *started with*. Nothing
is wrong with the field; the name invites exactly the reading I gave it, and I gave it that
reading while writing up an investigation about the pool being full.

## Gate state

`cargo fmt --all --check` clean. `orbistoun-kernel` alone: clippy `-D warnings` clean, 110 tests
pass. The **workspace** clippy and test gates are red, and not from this work - another session's
in-flight `orbistoun-gpu-vulkan` edits do not currently compile (`compute.rs` `Properties` gained
a fourth bool; `framebuffer.rs:1550` returns the wrong type). Left alone.
